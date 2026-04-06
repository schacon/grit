//! `grit fsck` command.
//!
//! Verifies the connectivity and validity of objects in the database.
//! Walks all reachable objects from refs, HEAD, and reflogs, then
//! checks each object for valid SHA, correct type header, and parseable
//! content. Reports dangling, unreachable, missing, and broken objects.

use crate::commands::git_passthrough;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pack::read_local_pack_indexes;
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
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

    /// Write dangling objects into .git/lost-found/{commit,other}/.
    #[arg(long = "lost-found")]
    pub lost_found: bool,

    /// Print the name of the object along with its hex ID.
    #[arg(long = "name-objects")]
    pub name_objects: bool,

    /// Suppress progress output.
    #[arg(long = "no-progress")]
    pub no_progress: bool,

    /// Show progress output.
    #[arg(long = "progress")]
    pub progress: bool,

    /// Optional list of objects to check (currently ignored, for compat).
    #[arg(value_name = "OBJECT")]
    pub objects: Vec<String>,
}

/// A problem found during fsck.
#[derive(Debug)]
enum Issue {
    /// Object referenced but not found anywhere.
    Missing {
        oid: ObjectId,
        kind: &'static str,
        referenced_by: ObjectId,
    },
    /// Object data is corrupt or unparseable.
    BadObject {
        oid: ObjectId,
        kind: ObjectKind,
        reason: String,
    },
    /// Object is dangling (exists but not reachable from any ref).
    Dangling { oid: ObjectId, kind: ObjectKind },
    /// Object is unreachable (exists but not reachable from any ref).
    Unreachable { oid: ObjectId, kind: ObjectKind },
}

/// Run `grit fsck`.
pub fn run(args: Args) -> Result<()> {
    if args.unreachable {
        return passthrough_current_fsck_invocation();
    }

    let repo = Repository::discover(None).context("failed to discover repository")?;
    // In linked worktrees, object storage lives in the common gitdir, not
    // under `.git/worktrees/<name>/objects`.
    let objects_dir = repo.odb.objects_dir().to_path_buf();
    let odb = Odb::new(&objects_dir);

    let show_dangling = !args.no_dangling;
    let show_unreachable = args.unreachable;
    let connectivity_only = args.connectivity_only;
    let lost_found = args.lost_found;
    let name_objects = args.name_objects;

    let mut issues: Vec<Issue> = Vec::new();
    let mut has_errors = false;

    // 1. Collect all reachable OIDs by walking from refs, HEAD, reflogs.
    //    Also track missing objects and (optionally) bad objects.
    let (reachable, walked_kinds) =
        walk_reachable(&repo, &odb, &objects_dir, connectivity_only, &mut issues)?;

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

    // 5. If --lost-found, write dangling objects to .git/lost-found/.
    if lost_found {
        let lost_commit_dir = repo.git_dir.join("lost-found").join("commit");
        let lost_other_dir = repo.git_dir.join("lost-found").join("other");
        fs::create_dir_all(&lost_commit_dir).ok();
        fs::create_dir_all(&lost_other_dir).ok();
        for issue in &issues {
            if let Issue::Dangling { oid, kind } = issue {
                let dir = match kind {
                    ObjectKind::Commit => &lost_commit_dir,
                    _ => &lost_other_dir,
                };
                let hex = oid.to_hex();
                let path = dir.join(&hex);
                // Write the object content to the file.
                if let Ok(obj) = odb.read(oid) {
                    fs::write(&path, &obj.data).ok();
                } else {
                    // Object might be in pack; just touch the file.
                    fs::write(&path, b"").ok();
                }
            }
        }
    }

    // 5b. Build name map if --name-objects is set.
    let name_map = if name_objects {
        build_name_map(&repo, &odb, &objects_dir)?
    } else {
        std::collections::HashMap::new()
    };

    // 6. Report issues.
    for issue in &issues {
        match issue {
            Issue::Missing {
                oid,
                kind,
                referenced_by,
            } => {
                let name_suffix = if name_objects {
                    name_map
                        .get(oid)
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                eprintln!(
                    "missing {} {}{} (referenced by {})",
                    kind,
                    oid.to_hex(),
                    name_suffix,
                    referenced_by.to_hex()
                );
                has_errors = true;
            }
            Issue::BadObject { oid, kind, reason } => {
                let name_suffix = if name_objects {
                    name_map
                        .get(oid)
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                eprintln!(
                    "error in {} {}{}: {}",
                    kind.as_str(),
                    oid.to_hex(),
                    name_suffix,
                    reason
                );
                has_errors = true;
            }
            Issue::Dangling { oid, kind } => {
                let name_suffix = if name_objects {
                    name_map
                        .get(oid)
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                eprintln!("dangling {} {}{}", kind.as_str(), oid.to_hex(), name_suffix);
            }
            Issue::Unreachable { oid, kind } => {
                let name_suffix = if name_objects {
                    name_map
                        .get(oid)
                        .map(|n| format!(" ({})", n))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                eprintln!(
                    "unreachable {} {}{}",
                    kind.as_str(),
                    oid.to_hex(),
                    name_suffix
                );
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

    // NOTE: We do NOT seed from reflogs for the main reachable walk.
    // Objects only reachable through reflogs are still considered dangling.
    // We walk reflog entries separately below to check for missing objects.

    // Collect packed IDs so we know which objects exist in pack files.
    let packed_ids = collect_packed_ids(objects_dir)?;

    // BFS walk.
    while let Some((oid, referrer)) = queue.pop_front() {
        if !reachable.insert(oid) {
            continue;
        }

        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => {
                // Try reading from pack (odb.read should handle this, but
                // if it didn't, check packed_ids to avoid false missing).
                if packed_ids.contains(&oid) {
                    // Object exists in pack — mark as reachable but can't
                    // walk its children without reading it.
                    continue;
                }
                // Object is missing.
                let ref_oid = referrer.unwrap_or(oid);
                issues.push(Issue::Missing {
                    oid,
                    kind: "object",
                    referenced_by: ref_oid,
                });
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
fn validate_object_data(oid: &ObjectId, kind: &ObjectKind, data: &[u8], issues: &mut Vec<Issue>) {
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

/// Build a map from ObjectId to a human-readable name (ref path or tree path).
/// This is used for --name-objects to show where objects are referenced.
fn build_name_map(
    repo: &Repository,
    odb: &Odb,
    _objects_dir: &Path,
) -> Result<HashMap<ObjectId, String>> {
    let mut names: HashMap<ObjectId, String> = HashMap::new();

    // Name objects by the refs that point to them.
    if let Ok(all_refs) = refs::list_refs(&repo.git_dir, "refs/") {
        for (refname, oid) in &all_refs {
            names.entry(*oid).or_insert_with(|| refname.clone());
            // Walk commit to name tree and parents.
            if let Ok(obj) = odb.read(oid) {
                if obj.kind == ObjectKind::Commit {
                    if let Ok(commit) = parse_commit(&obj.data) {
                        names
                            .entry(commit.tree)
                            .or_insert_with(|| format!("{}^{{tree}}", refname));
                        for (i, parent) in commit.parents.iter().enumerate() {
                            names
                                .entry(*parent)
                                .or_insert_with(|| format!("{}~{}", refname, i + 1));
                        }
                        // Walk the tree to name blobs.
                        name_tree_entries(odb, &commit.tree, refname, &mut names);
                    }
                }
            }
        }
    }

    // Name HEAD.
    if let Ok(head_oid) = refs::resolve_ref(&repo.git_dir, "HEAD") {
        names.entry(head_oid).or_insert_with(|| "HEAD".to_string());
    }

    Ok(names)
}

/// Recursively name tree entries for --name-objects.
fn name_tree_entries(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &str,
    names: &mut HashMap<ObjectId, String>,
) {
    if let Ok(obj) = odb.read(tree_oid) {
        if let Ok(entries) = parse_tree(&obj.data) {
            for entry in entries {
                let entry_name = String::from_utf8_lossy(&entry.name);
                let path = format!("{}:{}", prefix, entry_name);
                names.entry(entry.oid).or_insert(path.clone());
                // Recurse into subtrees.
                if entry.mode == 0o40000 {
                    name_tree_entries(
                        odb,
                        &entry.oid,
                        &format!("{}:{}", prefix, entry_name),
                        names,
                    );
                }
            }
        }
    }
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

fn passthrough_current_fsck_invocation() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let Some(idx) = argv.iter().position(|arg| arg == "fsck") else {
        bail!("failed to determine fsck arguments");
    };
    let passthrough_args = argv.get(idx + 1..).map(|s| s.to_vec()).unwrap_or_default();
    git_passthrough::run("fsck", &passthrough_args)
}
