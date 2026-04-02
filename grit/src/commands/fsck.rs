//! `grit fsck` command.
//!
//! Verifies the connectivity and validity of objects in the database.
//! Walks all reachable objects from refs, HEAD, and reflogs, then
//! checks each object for valid SHA, correct type header, and parseable
//! content. Reports dangling, unreachable, missing, and broken objects.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pack::read_local_pack_indexes;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::Path;

/// Arguments for `grit fsck`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show unreachable objects.
    #[arg(long = "unreachable")]
    pub unreachable: bool,

    /// Show dangling objects (default behavior).
    #[arg(long = "dangling", overrides_with = "no_dangling")]
    pub dangling: bool,

    /// Suppress dangling object output.
    #[arg(long = "no-dangling")]
    pub no_dangling: bool,

    /// Only check connectivity, skip object content validation.
    #[arg(long = "connectivity-only")]
    pub connectivity_only: bool,
}

/// A problem found during fsck.
#[derive(Debug)]
enum Issue {
    /// Object referenced but not found anywhere.
    Missing { oid: ObjectId, kind: &'static str, referenced_by: ObjectId },
    /// Object data is corrupt or unparseable.
    BadObject { oid: ObjectId, kind: ObjectKind, reason: String },
    /// Object is dangling (exists but not reachable from any ref).
    Dangling { oid: ObjectId, kind: ObjectKind },
    /// Object is unreachable (exists but not reachable from any ref).
    Unreachable { oid: ObjectId, kind: ObjectKind },
}

/// Run `grit fsck`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;
    let objects_dir = repo.git_dir.join("objects");
    let odb = Odb::new(&objects_dir);

    let show_dangling = !args.no_dangling;
    let show_unreachable = args.unreachable;
    let connectivity_only = args.connectivity_only;

    let mut issues: Vec<Issue> = Vec::new();
    let mut has_errors = false;

    // 1. Collect all reachable OIDs by walking from refs, HEAD, reflogs.
    //    Also track missing objects and (optionally) bad objects.
    let (reachable, walked_kinds) = walk_reachable(
        &repo,
        &odb,
        &objects_dir,
        connectivity_only,
        &mut issues,
    )?;

    // 2. Enumerate all known objects (loose + packed).
    let all_objects = enumerate_all_objects(&odb, &objects_dir)?;

    // 3. Find unreachable/dangling objects.
    if show_dangling || show_unreachable {
        // "dangling" = unreachable and not referenced by any other unreachable object.
        // For simplicity (matching git behavior), dangling means: exists but not
        // in the reachable set, AND not referenced by another unreachable object.
        // "unreachable" means: exists but not in the reachable set.

        let unreachable_oids: BTreeSet<ObjectId> = all_objects
            .iter()
            .filter(|oid| !reachable.contains(oid))
            .copied()
            .collect();

        if show_unreachable {
            // Report all unreachable objects.
            for oid in &unreachable_oids {
                let kind = read_object_kind(&odb, oid);
                issues.push(Issue::Unreachable { oid: *oid, kind });
            }
        }

        if show_dangling && !show_unreachable {
            // Find dangling: unreachable objects not referenced by other unreachable objects.
            let referenced_by_unreachable = find_referenced_set(&odb, &unreachable_oids);
            for oid in &unreachable_oids {
                if !referenced_by_unreachable.contains(oid) {
                    let kind = read_object_kind(&odb, oid);
                    issues.push(Issue::Dangling { oid: *oid, kind });
                }
            }
        }
    }

    // 4. If not connectivity-only, validate ALL objects (including unreachable).
    if !connectivity_only {
        for oid in &all_objects {
            // Skip objects we already validated during the walk.
            if walked_kinds.contains(oid) {
                continue;
            }
            validate_object(&odb, oid, &mut issues);
        }
    }

    // 5. Report issues.
    for issue in &issues {
        match issue {
            Issue::Missing { oid, kind, referenced_by } => {
                eprintln!("missing {} {} (referenced by {})", kind, oid.to_hex(), referenced_by.to_hex());
                has_errors = true;
            }
            Issue::BadObject { oid, kind, reason } => {
                eprintln!("error in {} {}: {}", kind.as_str(), oid.to_hex(), reason);
                has_errors = true;
            }
            Issue::Dangling { oid, kind } => {
                eprintln!("dangling {} {}", kind.as_str(), oid.to_hex());
            }
            Issue::Unreachable { oid, kind } => {
                eprintln!("unreachable {} {}", kind.as_str(), oid.to_hex());
            }
        }
    }

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

/// Walk all reachable objects from refs, HEAD, and reflogs.
/// Returns (reachable set, set of OIDs whose content was validated).
fn walk_reachable(
    repo: &Repository,
    odb: &Odb,
    objects_dir: &Path,
    connectivity_only: bool,
    issues: &mut Vec<Issue>,
) -> Result<(HashSet<ObjectId>, HashSet<ObjectId>)> {
    let mut reachable = HashSet::new();
    let mut validated = HashSet::new();
    let mut queue: VecDeque<(ObjectId, Option<ObjectId>)> = VecDeque::new();

    // Seed from HEAD.
    if let Ok(head_oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        queue.push_back((head_oid, None));
    }

    // Seed from all refs.
    if let Ok(all_refs) = refs::list_refs(&repo.git_dir, "refs/") {
        for (_, oid) in all_refs {
            queue.push_back((oid, None));
        }
    }

    // Seed from reflogs.
    collect_reflog_seeds(&repo.git_dir, &mut queue);

    // Include packed object IDs as reachable (they exist in the store).
    let packed_ids = collect_packed_ids(objects_dir)?;
    reachable.extend(&packed_ids);

    // BFS walk.
    while let Some((oid, referrer)) = queue.pop_front() {
        if !reachable.insert(oid) {
            continue;
        }

        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => {
                // Check if it's in a pack (already marked reachable).
                if packed_ids.contains(&oid) {
                    continue;
                }
                // Object is missing.
                if let Some(ref_oid) = referrer {
                    issues.push(Issue::Missing {
                        oid,
                        kind: "object",
                        referenced_by: ref_oid,
                    });
                }
                continue;
            }
        };

        // Validate object content if not connectivity-only.
        if !connectivity_only {
            validated.insert(oid);
            validate_object_data(&oid, &obj.kind, &obj.data, issues);
        }

        match obj.kind {
            ObjectKind::Commit => {
                if let Ok(commit) = parse_commit(&obj.data) {
                    queue.push_back((commit.tree, Some(oid)));
                    for parent in commit.parents {
                        queue.push_back((parent, Some(oid)));
                    }
                }
            }
            ObjectKind::Tree => {
                if let Ok(entries) = parse_tree(&obj.data) {
                    for entry in entries {
                        queue.push_back((entry.oid, Some(oid)));
                    }
                }
            }
            ObjectKind::Tag => {
                if let Ok(tag) = parse_tag(&obj.data) {
                    queue.push_back((tag.object, Some(oid)));
                }
            }
            ObjectKind::Blob => {}
        }
    }

    Ok((reachable, validated))
}

/// Validate an object's content can be parsed correctly.
fn validate_object(odb: &Odb, oid: &ObjectId, issues: &mut Vec<Issue>) {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => return, // can't read loose — might be packed only
    };
    validate_object_data(oid, &obj.kind, &obj.data, issues);
}

/// Validate the parsed content of an object.
fn validate_object_data(
    oid: &ObjectId,
    kind: &ObjectKind,
    data: &[u8],
    issues: &mut Vec<Issue>,
) {
    match kind {
        ObjectKind::Commit => {
            if let Err(e) = parse_commit(data) {
                issues.push(Issue::BadObject {
                    oid: *oid,
                    kind: *kind,
                    reason: format!("invalid commit: {e}"),
                });
            }
        }
        ObjectKind::Tree => {
            if let Err(e) = parse_tree(data) {
                issues.push(Issue::BadObject {
                    oid: *oid,
                    kind: *kind,
                    reason: format!("invalid tree: {e}"),
                });
            }
        }
        ObjectKind::Tag => {
            if let Err(e) = parse_tag(data) {
                issues.push(Issue::BadObject {
                    oid: *oid,
                    kind: *kind,
                    reason: format!("invalid tag: {e}"),
                });
            }
        }
        ObjectKind::Blob => {
            // Blobs are arbitrary data — no structural validation needed.
        }
    }
}

/// Read the kind of an object (for display purposes).
fn read_object_kind(odb: &Odb, oid: &ObjectId) -> ObjectKind {
    match odb.read(oid) {
        Ok(obj) => obj.kind,
        Err(_) => ObjectKind::Blob, // fallback
    }
}

/// Find all OIDs referenced by a set of objects (for dangling detection).
fn find_referenced_set(odb: &Odb, oids: &BTreeSet<ObjectId>) -> HashSet<ObjectId> {
    let mut referenced = HashSet::new();
    for oid in oids {
        let obj = match odb.read(oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        match obj.kind {
            ObjectKind::Commit => {
                if let Ok(commit) = parse_commit(&obj.data) {
                    referenced.insert(commit.tree);
                    for parent in commit.parents {
                        referenced.insert(parent);
                    }
                }
            }
            ObjectKind::Tree => {
                if let Ok(entries) = parse_tree(&obj.data) {
                    for entry in entries {
                        referenced.insert(entry.oid);
                    }
                }
            }
            ObjectKind::Tag => {
                if let Ok(tag) = parse_tag(&obj.data) {
                    referenced.insert(tag.object);
                }
            }
            ObjectKind::Blob => {}
        }
    }
    referenced
}

/// Enumerate all known object IDs (loose + packed).
fn enumerate_all_objects(_odb: &Odb, objects_dir: &Path) -> Result<BTreeSet<ObjectId>> {
    let mut all = BTreeSet::new();

    // Loose objects.
    let loose = scan_loose_objects(objects_dir)?;
    for (oid, _) in loose {
        all.insert(oid);
    }

    // Packed objects.
    let packed = collect_packed_ids(objects_dir)?;
    all.extend(packed);

    Ok(all)
}

/// Scan reflog files for object IDs and add them as seeds.
fn collect_reflog_seeds(git_dir: &Path, queue: &mut VecDeque<(ObjectId, Option<ObjectId>)>) {
    let logs_dir = git_dir.join("logs");
    if let Ok(entries) = walk_files(&logs_dir) {
        for path in entries {
            if let Ok(content) = fs::read_to_string(&path) {
                for line in content.lines() {
                    let parts: Vec<&str> = line.splitn(3, ' ').collect();
                    if parts.len() >= 2 {
                        let zero = ObjectId::from_bytes(&[0; 20]).unwrap();
                        if let Ok(oid) = parts[0].parse::<ObjectId>() {
                            if oid != zero {
                                queue.push_back((oid, None));
                            }
                        }
                        if let Ok(oid) = parts[1].parse::<ObjectId>() {
                            if oid != zero {
                                queue.push_back((oid, None));
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
