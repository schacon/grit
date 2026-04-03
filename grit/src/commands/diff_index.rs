//! `grit diff-index` command.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{detect_copies, detect_renames, stat_matches, DiffEntry, DiffStatus, zero_oid};
use std::io::Write;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_REGULAR, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{abbreviate_object_id, resolve_revision};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Arguments for `grit diff-index`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit diff-index`.
pub fn run(args: Args) -> Result<()> {
    let options = parse_options(&args.args)?;
    let repo = Repository::discover(None).context("not a git repository")?;
    let tree_oid = resolve_tree_ish(&repo, &options.tree_ish)?;

    let mut tree_map = BTreeMap::new();
    collect_tree_entries(&repo, &tree_oid, "", &mut tree_map)?;

    let index_path = effective_index_path(&repo)?;
    let index = Index::load(&index_path).context("loading index")?;
    let mut index_map = BTreeMap::new();
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if let Ok(path) = String::from_utf8(entry.path.clone()) {
            if matches_pathspec(&path, &options.pathspecs) {
                index_map.insert(path, Snapshot::from_index_entry(entry.mode, entry.oid));
            }
        }
    }
    tree_map.retain(|path, _| matches_pathspec(path, &options.pathspecs));

    let changes = if options.cached {
        diff_tree_vs_index(&tree_map, &index_map)
    } else {
        diff_tree_vs_worktree(&repo, &tree_map, &index_map, &index, options.match_missing)?
    };

    // Convert to DiffEntry for rename detection and output.
    let diff_entries: Vec<DiffEntry> = changes
        .iter()
        .map(|c| raw_change_to_diff_entry(c))
        .collect();

    let diff_entries = if options.find_copies {
        let threshold = options.find_renames.unwrap_or(50);
        // Build source tree entries for copy detection.
        let source_tree_entries: Vec<(String, String, ObjectId)> = tree_map
            .iter()
            .map(|(path, snap)| (path.clone(), format!("{:06o}", snap.mode), snap.oid))
            .collect();
        detect_copies(
            &repo.odb,
            diff_entries,
            threshold,
            options.find_copies_harder,
            &source_tree_entries,
        )
    } else if let Some(threshold) = options.find_renames {
        detect_renames(&repo.odb, diff_entries, threshold)
    } else {
        diff_entries
    };

    if !options.quiet {
        if options.patch {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            for entry in &diff_entries {
                write_patch_entry(&mut out, &repo.odb, entry)?;
            }
        } else if options.name_status {
            for entry in &diff_entries {
                match (entry.status, entry.score) {
                    (DiffStatus::Renamed, Some(s)) => {
                        println!("R{:03}\t{}\t{}",
                            s,
                            entry.old_path.as_deref().unwrap_or(""),
                            entry.new_path.as_deref().unwrap_or(""));
                    }
                    (DiffStatus::Copied, Some(s)) => {
                        println!("C{:03}\t{}\t{}",
                            s,
                            entry.old_path.as_deref().unwrap_or(""),
                            entry.new_path.as_deref().unwrap_or(""));
                    }
                    _ => {
                        println!("{}\t{}", entry.status.letter(), entry.path());
                    }
                }
            }
        } else {
            let terminator = if options.nul_terminated { "\0" } else { "\n" };
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            for entry in &diff_entries {
                let line = render_raw_diff_entry(entry, &repo, options.abbrev)?;
                if options.nul_terminated {
                    // In -z mode, the colon-prefixed status line ends with NUL,
                    // then path(s) follow separated/terminated by NUL
                    write!(out, "{}{}", line, terminator)?;
                } else {
                    writeln!(out, "{}", line)?;
                }
            }
        }
    }

    if (options.exit_code || options.quiet) && !diff_entries.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Options {
    tree_ish: String,
    pathspecs: Vec<String>,
    cached: bool,
    match_missing: bool,
    quiet: bool,
    exit_code: bool,
    abbrev: Option<usize>,
    find_renames: Option<u32>,
    find_copies: bool,
    find_copies_harder: bool,
    patch: bool,
    name_status: bool,
    nul_terminated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Snapshot {
    mode: u32,
    oid: ObjectId,
}

impl Snapshot {
    fn from_index_entry(mode: u32, oid: ObjectId) -> Self {
        Self {
            mode: canonicalize_mode(mode),
            oid,
        }
    }
}

#[derive(Debug, Clone)]
struct RawChange {
    path: String,
    status: char,
    old: Option<Snapshot>,
    new: Option<Snapshot>,
}

fn parse_options(argv: &[String]) -> Result<Options> {
    let mut cached = false;
    let mut match_missing = false;
    let mut quiet = false;
    let mut exit_code = false;
    let mut abbrev: Option<usize> = None;
    let mut tree_ish: Option<String> = None;
    let mut pathspecs = Vec::new();
    let mut end_of_options = false;
    let mut find_renames: Option<u32> = None;
    let mut find_copies = false;
    let mut find_copies_harder = false;
    let mut c_count = 0u32;
    let mut patch = false;
    let mut name_status = false;
    let mut nul_terminated = false;

    let mut idx = 0usize;
    while idx < argv.len() {
        let arg = &argv[idx];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            idx += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') {
            match arg.as_str() {
                "--cached" => cached = true,
                "-m" => match_missing = true,
                "--quiet" => quiet = true,
                "--exit-code" => exit_code = true,
                "--raw" => {}
                "--abbrev" => abbrev = Some(7),
                "-p" | "--patch" | "-u" => {
                    patch = true;
                }
                "--name-status" => {
                    name_status = true;
                }
                "-M" | "--find-renames" => {
                    find_renames = Some(50);
                }
                "--no-renames" => {
                    find_renames = None;
                }
                _ if arg.starts_with("-M") => {
                    let val = &arg[2..];
                    let pct = if val.ends_with('%') {
                        val[..val.len()-1].parse::<u32>().unwrap_or(50)
                    } else {
                        val.parse::<u32>().unwrap_or(50)
                    };
                    find_renames = Some(pct);
                }
                _ if arg.starts_with("--find-renames=") => {
                    let val = &arg["--find-renames=".len()..];
                    let pct = if val.ends_with('%') {
                        val[..val.len()-1].parse::<u32>().unwrap_or(50)
                    } else {
                        val.parse::<u32>().unwrap_or(50)
                    };
                    find_renames = Some(pct);
                }
                _ if arg.starts_with("-l") && arg[2..].parse::<usize>().is_ok() => {
                    // rename limit - accept and ignore for now
                }
                "-r" => {
                    // recursive - default behavior for diff-index
                }
                "-z" => {
                    nul_terminated = true;
                }
                "-C" | "--find-copies" => {
                    c_count += 1;
                    find_copies = true;
                    if c_count >= 2 {
                        find_copies_harder = true;
                    }
                    if find_renames.is_none() {
                        find_renames = Some(50);
                    }
                }
                "--find-copies-harder" => {
                    find_copies = true;
                    find_copies_harder = true;
                    if find_renames.is_none() {
                        find_renames = Some(50);
                    }
                }
                _ if arg.starts_with("--abbrev=") => {
                    let value = arg.trim_start_matches("--abbrev=");
                    let parsed = value
                        .parse::<usize>()
                        .with_context(|| format!("invalid --abbrev value: `{value}`"))?;
                    abbrev = Some(parsed);
                }
                _ => bail!("unsupported option: {arg}"),
            }
            idx += 1;
            continue;
        }

        if tree_ish.is_none() {
            tree_ish = Some(arg.clone());
        } else {
            pathspecs.push(arg.clone());
        }
        idx += 1;
    }

    let Some(tree_ish) = tree_ish else {
        bail!("usage: grit diff-index [-m] [--cached] [--raw] [--quiet] [--exit-code] [--abbrev[=<n>]] <tree-ish> [<path>...]");
    };

    Ok(Options {
        tree_ish,
        pathspecs,
        cached,
        match_missing,
        quiet,
        exit_code,
        abbrev,
        find_renames,
        find_copies,
        find_copies_harder,
        patch,
        name_status,
        nul_terminated,
    })
}

/// Resolve a revision to a tree OID without redundantly reading the tree object.
/// Returns the tree OID which can be passed directly to collect_tree_entries.
fn resolve_tree_ish(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let oid = resolve_revision(repo, spec)?;
    // Peel commits to get their tree OID without reading the tree itself.
    // The tree will be read later by collect_tree_entries.
    loop {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data)?;
                // Return the tree OID directly — don't read/verify the tree
                // object here, collect_tree_entries will do that.
                return Ok(commit.tree);
            }
            _ => bail!("object '{}' does not name a tree", oid),
        }
    }
}

fn collect_tree_entries(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &str,
    out: &mut BTreeMap<String, Snapshot>,
) -> Result<()> {
    let obj = repo.odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree object");
    }
    for entry in parse_tree(&obj.data)? {
        let name = String::from_utf8(entry.name)
            .map_err(|_| anyhow::anyhow!("tree contains non-UTF-8 path"))?;
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode == 0o040000 {
            collect_tree_entries(repo, &entry.oid, &path, out)?;
        } else {
            out.insert(path, Snapshot::from_index_entry(entry.mode, entry.oid));
        }
    }
    Ok(())
}

fn diff_tree_vs_index(
    tree_map: &BTreeMap<String, Snapshot>,
    index_map: &BTreeMap<String, Snapshot>,
) -> Vec<RawChange> {
    let mut all_paths = BTreeSet::new();
    all_paths.extend(tree_map.keys().cloned());
    all_paths.extend(index_map.keys().cloned());

    let mut changes = Vec::new();
    for path in all_paths {
        let old = tree_map.get(&path).copied();
        let new = index_map.get(&path).copied();
        match (old, new) {
            (Some(old), Some(new)) if old == new => {}
            (Some(old), Some(new)) => changes.push(RawChange {
                path,
                status: 'M',
                old: Some(old),
                new: Some(new),
            }),
            (Some(old), None) => changes.push(RawChange {
                path,
                status: 'D',
                old: Some(old),
                new: None,
            }),
            (None, Some(new)) => changes.push(RawChange {
                path,
                status: 'A',
                old: None,
                new: Some(new),
            }),
            (None, None) => {}
        }
    }
    changes
}

fn diff_tree_vs_worktree(
    repo: &Repository,
    tree_map: &BTreeMap<String, Snapshot>,
    index_map: &BTreeMap<String, Snapshot>,
    index: &Index,
    match_missing: bool,
) -> Result<Vec<RawChange>> {
    let Some(work_tree) = &repo.work_tree else {
        bail!("this operation must be run in a work tree");
    };

    // Build a lookup from path → index entry for stat cache checks
    let index_entries: BTreeMap<&[u8], &IndexEntry> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    let mut merged = BTreeMap::new();
    for change in diff_tree_vs_index(tree_map, index_map) {
        merged.insert(change.path.clone(), change);
    }

    for (path, index_snapshot) in index_map {
        let abs = work_tree.join(path);

        // Fast path: use stat cache to skip unchanged files
        let meta = match fs::symlink_metadata(&abs) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if match_missing {
                    continue;
                }
                let old = tree_map.get(path).copied().or(Some(*index_snapshot));
                merged.insert(
                    path.clone(),
                    RawChange {
                        path: path.clone(),
                        status: 'D',
                        old,
                        new: None,
                    },
                );
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        // Check stat cache — if stat matches index entry, file is unchanged
        if let Some(ie) = index_entries.get(path.as_bytes()) {
            if stat_matches(ie, &meta) {
                continue; // Fast path: stat data matches, skip hashing
            }
        }

        // Stat differs — must read and hash the file
        match read_worktree_snapshot_from_meta(repo, &abs, &meta)? {
            Some(worktree_snapshot) => {
                if worktree_snapshot != *index_snapshot {
                    let old = tree_map.get(path).copied();
                    merged.insert(
                        path.clone(),
                        RawChange {
                            path: path.clone(),
                            status: 'M',
                            old,
                            new: None,
                        },
                    );
                }
            }
            None => {
                // Not a regular file or symlink — treat as missing
            }
        }
    }

    Ok(merged.into_values().collect())
}

fn read_worktree_snapshot_from_meta(_repo: &Repository, abs_path: &Path, metadata: &fs::Metadata) -> Result<Option<Snapshot>> {
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(abs_path)?;
        let oid = Odb::hash_object_data(ObjectKind::Blob, target.as_os_str().as_bytes());
        return Ok(Some(Snapshot {
            mode: MODE_SYMLINK,
            oid,
        }));
    }

    if metadata.file_type().is_file() {
        let mode = canonicalize_mode(metadata.permissions().mode());
        let data = fs::read(abs_path)?;
        let oid = Odb::hash_object_data(ObjectKind::Blob, &data);
        return Ok(Some(Snapshot { mode, oid }));
    }

    Ok(None)
}

fn canonicalize_mode(raw_mode: u32) -> u32 {
    match raw_mode & 0o170000 {
        0o120000 => MODE_SYMLINK,
        0o160000 => MODE_GITLINK,
        0o100000 => {
            if raw_mode & 0o111 != 0 {
                MODE_EXECUTABLE
            } else {
                MODE_REGULAR
            }
        }
        _ => MODE_REGULAR,
    }
}

fn matches_pathspec(path: &str, pathspecs: &[String]) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    pathspecs.iter().any(|spec| {
        if let Some(prefix) = spec.strip_suffix('/') {
            path == prefix || path.starts_with(&format!("{prefix}/"))
        } else {
            path == spec || path.starts_with(&format!("{spec}/"))
        }
    })
}

fn effective_index_path(repo: &Repository) -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Ok(path);
        }
        let cwd = std::env::current_dir().context("resolving GIT_INDEX_FILE")?;
        return Ok(cwd.join(path));
    }
    Ok(repo.index_path())
}

/// Convert a RawChange to a DiffEntry for rename detection.
fn raw_change_to_diff_entry(change: &RawChange) -> DiffEntry {
    let status = match change.status {
        'A' => DiffStatus::Added,
        'D' => DiffStatus::Deleted,
        'M' => DiffStatus::Modified,
        'R' => DiffStatus::Renamed,
        'C' => DiffStatus::Copied,
        'T' => DiffStatus::TypeChanged,
        _ => DiffStatus::Modified,
    };

    let old_mode = change.old.map_or(0, |s| s.mode);
    let new_mode = change.new.map_or(0, |s| s.mode);

    DiffEntry {
        status,
        old_path: if change.status == 'A' { None } else { Some(change.path.clone()) },
        new_path: if change.status == 'D' { None } else { Some(change.path.clone()) },
        old_mode: format!("{old_mode:06o}"),
        new_mode: format!("{new_mode:06o}"),
        old_oid: change.old.map_or_else(zero_oid, |s| s.oid),
        new_oid: change.new.map_or_else(zero_oid, |s| s.oid),
        score: None,
    }
}

/// Render a DiffEntry in raw format.
fn render_raw_diff_entry(entry: &DiffEntry, repo: &Repository, abbrev: Option<usize>) -> Result<String> {
    let width = abbrev.unwrap_or(40).clamp(4, 40);

    let old_oid = if entry.old_oid == zero_oid() {
        "0".repeat(width)
    } else {
        match abbrev {
            Some(min_len) => abbreviate_object_id(repo, entry.old_oid, min_len)?,
            None => entry.old_oid.to_hex(),
        }
    };
    let new_oid = if entry.new_oid == zero_oid() {
        "0".repeat(width)
    } else {
        match abbrev {
            Some(min_len) => abbreviate_object_id(repo, entry.new_oid, min_len)?,
            None => entry.new_oid.to_hex(),
        }
    };

    let status_str = match (entry.status, entry.score) {
        (DiffStatus::Renamed, Some(s)) => format!("R{:03}", s),
        (DiffStatus::Copied, Some(s)) => format!("C{:03}", s),
        _ => entry.status.letter().to_string(),
    };

    let path = match entry.status {
        DiffStatus::Renamed | DiffStatus::Copied => {
            format!(
                "{}\t{}",
                entry.old_path.as_deref().unwrap_or(""),
                entry.new_path.as_deref().unwrap_or("")
            )
        }
        _ => entry.path().to_owned(),
    };

    Ok(format!(
        ":{} {} {} {} {}\t{}",
        entry.old_mode, entry.new_mode, old_oid, new_oid, status_str, path
    ))
}

/// Write a unified-diff block for one entry (diff-index -p).
fn write_patch_entry(
    out: &mut impl std::io::Write,
    odb: &Odb,
    entry: &DiffEntry,
) -> Result<()> {
    use grit_lib::diff::unified_diff;

    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    writeln!(out, "diff --git a/{old_path} b/{new_path}")?;

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "new file mode {}", entry.new_mode)?;
            writeln!(out, "index {}..{}",
                &entry.old_oid.to_hex()[..7],
                &entry.new_oid.to_hex()[..7])?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "deleted file mode {}", entry.old_mode)?;
            writeln!(out, "index {}..{}",
                &entry.old_oid.to_hex()[..7],
                &entry.new_oid.to_hex()[..7])?;
        }
        DiffStatus::Modified => {
            if entry.old_mode == entry.new_mode {
                writeln!(out, "index {}..{} {}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7],
                    entry.old_mode)?;
            } else {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
                writeln!(out, "index {}..{}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7])?;
            }
        }
        DiffStatus::Renamed => {
            let sim = entry.score.unwrap_or(100);
            writeln!(out, "similarity index {sim}%")?;
            writeln!(out, "rename from {old_path}")?;
            writeln!(out, "rename to {new_path}")?;
            if entry.old_oid != entry.new_oid {
                writeln!(out, "index {}..{}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7])?;
            }
        }
        DiffStatus::Copied => {
            let sim = entry.score.unwrap_or(100);
            writeln!(out, "similarity index {sim}%")?;
            writeln!(out, "copy from {old_path}")?;
            writeln!(out, "copy to {new_path}")?;
            if entry.old_oid != entry.new_oid {
                writeln!(out, "index {}..{}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7])?;
            }
        }
        _ => {}
    }

    let z = zero_oid();
    let old_content = if entry.old_oid == z {
        String::new()
    } else {
        match odb.read(&entry.old_oid) {
            Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
            Err(_) => String::new(),
        }
    };
    let new_content = if entry.new_oid == z {
        String::new()
    } else {
        match odb.read(&entry.new_oid) {
            Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
            Err(_) => String::new(),
        }
    };

    let display_old = if entry.status == DiffStatus::Added { "/dev/null" } else { old_path };
    let display_new = if entry.status == DiffStatus::Deleted { "/dev/null" } else { new_path };

    let context_lines = 3;
    let patch = unified_diff(&old_content, &new_content, display_old, display_new, context_lines);
    write!(out, "{patch}")?;

    Ok(())
}
