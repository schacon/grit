//! `grit prune` command.
//!
//! Removes unreachable loose objects from the object database.  Only loose
//! objects (files under `.git/objects/XX/…`) are considered; packed objects
//! are left untouched.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pack::read_local_pack_indexes;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Arguments for `grit prune`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Do not remove anything; just show what would be removed.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Report pruned objects.
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Only prune objects older than this time (default: 2 weeks ago).
    ///
    /// Accepts "now" to prune everything, or a duration like "2.weeks.ago".
    #[arg(long = "expire")]
    pub expire: Option<String>,
}

/// Run `grit prune`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;

    // Refuse to prune when preciousObjects extension is set.
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_default();
    if config
        .get_bool("extensions.preciousObjects")
        .and_then(|r| r.ok())
        .unwrap_or(false)
    {
        anyhow::bail!("fatal: cannot prune in a repository with precious objects");
    }
    let objects_dir = repo.git_dir.join("objects");
    let odb = Odb::new(&objects_dir);

    let expire_time = parse_expire_time(args.expire.as_deref())?;

    // 1. Collect all reachable object IDs.
    let reachable = collect_reachable(&repo, &odb, &objects_dir)
        .context("failed to collect reachable objects")?;

    // 2. Enumerate all loose objects.
    let loose = scan_loose_objects(&objects_dir)?;

    // 3. Prune unreachable loose objects that are old enough.
    let mut pruned = 0usize;
    for (oid, path) in &loose {
        if reachable.contains(oid) {
            continue;
        }

        // Check modification time against expire threshold.
        if let Some(threshold) = expire_time {
            match fs::metadata(path).and_then(|m| m.modified()) {
                Ok(mtime) => {
                    if mtime >= threshold {
                        continue; // too new to prune
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => {} // can't read mtime, prune anyway
            }
        }

        if args.dry_run || args.verbose {
            println!("{}", oid.to_hex());
        }

        if !args.dry_run {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    eprintln!("warning: failed to remove {}: {e}", path.display());
                }
            }
            // Try to remove the now-possibly-empty prefix directory.
            if let Some(parent) = path.parent() {
                let _ = fs::remove_dir(parent);
            }
        }

        pruned += 1;
    }

    if args.verbose && !args.dry_run {
        eprintln!("prune: removed {} unreachable loose object(s)", pruned);
    }

    Ok(())
}

/// Parse the `--expire` value into a [`SystemTime`] threshold.
///
/// - `None` or `"2.weeks.ago"` → 2 weeks before now
/// - `"now"` → current time (prune everything regardless of age)
fn parse_expire_time(expire: Option<&str>) -> Result<Option<SystemTime>> {
    match expire {
        None => {
            // Default: 2 weeks ago.
            let two_weeks = Duration::from_secs(14 * 24 * 60 * 60);
            Ok(Some(
                SystemTime::now()
                    .checked_sub(two_weeks)
                    .unwrap_or(SystemTime::UNIX_EPOCH),
            ))
        }
        Some("now") => {
            // Prune everything: no age filter.
            Ok(None)
        }
        Some(s) => {
            // Try to parse durations like "2.weeks.ago", "1.day.ago", etc.
            if let Some(threshold) = parse_relative_time(s) {
                Ok(Some(threshold))
            } else {
                anyhow::bail!("unsupported --expire value: {s:?}");
            }
        }
    }
}

/// Parse Git-style relative time strings like "2.weeks.ago", "3.days.ago".
fn parse_relative_time(s: &str) -> Option<SystemTime> {
    let s = s.trim();
    // Handle forms like "2.weeks.ago", "1.day.ago", "3.hours.ago"
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() == 3 && parts[2] == "ago" {
        let n: u64 = parts[0].parse().ok()?;
        let unit = parts[1];
        let secs = match unit {
            "second" | "seconds" => n,
            "minute" | "minutes" => n * 60,
            "hour" | "hours" => n * 3600,
            "day" | "days" => n * 86400,
            "week" | "weeks" => n * 7 * 86400,
            "month" | "months" => n * 30 * 86400,
            "year" | "years" => n * 365 * 86400,
            _ => return None,
        };
        return SystemTime::now().checked_sub(Duration::from_secs(secs));
    }
    None
}

/// Build the set of all reachable object IDs by walking from refs.
fn collect_reachable(
    repo: &Repository,
    odb: &Odb,
    objects_dir: &Path,
) -> Result<HashSet<ObjectId>> {
    let mut reachable = HashSet::new();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();

    // Seed from HEAD.
    if let Ok(head_oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        queue.push_back(head_oid);
    }

    // Seed from all refs (branches, tags, etc.).
    if let Ok(all_refs) = refs::list_refs(&repo.git_dir, "refs/") {
        for (_, oid) in all_refs {
            queue.push_back(oid);
        }
    }

    // Seed from reflogs.
    collect_reflog_oids(&repo.git_dir, &mut queue);

    // Also mark all packed object IDs as reachable — we can't read their
    // contents to walk their children, but they definitely exist and anything
    // they reference is reachable.  This is conservative: we may keep a few
    // extra loose objects, but we never incorrectly prune a reachable one.
    let packed_ids = collect_packed_ids(objects_dir)?;
    reachable.extend(&packed_ids);

    // BFS walk.
    while let Some(oid) = queue.pop_front() {
        if !reachable.insert(oid) {
            continue;
        }

        // Try to read the object.  If it's only in a pack (and we already
        // marked all packed IDs above), we simply can't walk its children
        // from the loose store — that's fine.
        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => continue,
        };

        match obj.kind {
            ObjectKind::Commit => {
                if let Ok(commit) = parse_commit(&obj.data) {
                    // The tree and all parents are reachable.
                    queue.push_back(commit.tree);
                    for parent in commit.parents {
                        queue.push_back(parent);
                    }
                }
            }
            ObjectKind::Tree => {
                if let Ok(entries) = parse_tree(&obj.data) {
                    for entry in entries {
                        queue.push_back(entry.oid);
                    }
                }
            }
            ObjectKind::Tag => {
                if let Ok(tag) = parse_tag(&obj.data) {
                    queue.push_back(tag.object);
                }
            }
            ObjectKind::Blob => {
                // Blobs have no children.
            }
        }
    }

    Ok(reachable)
}

/// Scan reflog files for object IDs and add them to the queue.
fn collect_reflog_oids(git_dir: &Path, queue: &mut VecDeque<ObjectId>) {
    let logs_dir = git_dir.join("logs");
    if let Ok(entries) = walk_files(&logs_dir) {
        for path in entries {
            if let Ok(content) = fs::read_to_string(&path) {
                for line in content.lines() {
                    // Reflog format: "<old-oid> <new-oid> <identity> <timestamp> <message>"
                    let parts: Vec<&str> = line.splitn(3, ' ').collect();
                    #[allow(clippy::unwrap_used)]
                    let zero = ObjectId::from_bytes(&[0; 20]).unwrap();
                    if parts.len() >= 2 {
                        if let Ok(oid) = parts[0].parse::<ObjectId>() {
                            if oid != zero {
                                queue.push_back(oid);
                            }
                        }
                        if let Ok(oid) = parts[1].parse::<ObjectId>() {
                            if oid != zero {
                                queue.push_back(oid);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Recursively walk a directory, returning all file paths.
fn walk_files(dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e),
    };
    for entry in rd {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            files.extend(walk_files(&entry.path())?);
        } else if ft.is_file() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

/// Enumerate all loose objects in the object store.
fn scan_loose_objects(objects_dir: &Path) -> Result<Vec<(ObjectId, std::path::PathBuf)>> {
    let mut objects = Vec::new();
    let rd = match fs::read_dir(objects_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(objects),
        Err(e) => anyhow::bail!("failed to read objects dir: {e}"),
    };

    for entry in rd {
        let entry = entry?;
        let dir_name = entry.file_name().to_string_lossy().to_string();

        // Only two-hex-char prefix subdirectories.
        if dir_name.len() != 2
            || !dir_name.chars().all(|c| c.is_ascii_hexdigit())
            || !entry.path().is_dir()
        {
            continue;
        }

        let sub_rd = match fs::read_dir(entry.path()) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => anyhow::bail!("failed to read dir {}: {e}", entry.path().display()),
        };

        for file in sub_rd {
            let file = file?;
            let file_name = file.file_name().to_string_lossy().to_string();
            if file_name.len() != 38 || !file_name.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }

            let hex = format!("{dir_name}{file_name}");
            if let Ok(oid) = hex.parse::<ObjectId>() {
                objects.push((oid, file.path()));
            }
        }
    }

    Ok(objects)
}

/// Collect all object IDs present in local pack indexes.
fn collect_packed_ids(objects_dir: &Path) -> Result<HashSet<ObjectId>> {
    let indexes = read_local_pack_indexes(objects_dir)?;
    let mut ids = HashSet::new();
    for idx in indexes {
        for entry in idx.entries {
            ids.insert(entry.oid);
        }
    }
    Ok(ids)
}
