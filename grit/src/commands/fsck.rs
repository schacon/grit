//! `grit fsck` command.
//!
//! Verifies the connectivity and validity of objects in the database.
//! Walks all reachable objects from refs, HEAD, and reflogs, then
//! checks each object for valid SHA, correct type header, and parseable
//! content. Reports dangling, unreachable, missing, and broken objects.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::explicit_exit::ExplicitExit;
use grit_lib::config::ConfigSet;
use grit_lib::diff::zero_oid;
use grit_lib::error::Error as LibError;
use grit_lib::fsck_standalone::{fsck_object, fsck_tag_mktag_trailer};
use grit_lib::ident::fsck_commit_idents;
use grit_lib::objects::{parse_commit, parse_tree, tag_object_line_oid, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::pack::read_local_pack_indexes;
use grit_lib::promisor::{
    promisor_expanded_object_ids, promisor_pack_object_ids, repo_treats_promisor_packs,
};
use grit_lib::reflog::{list_reflog_refs, read_reflog};
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::shallow::load_shallow_boundaries;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

    /// Full object database check (accepted for Git compatibility; grit verifies reachable and loose objects by default).
    #[arg(long = "full")]
    pub full: bool,

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

    /// Enable stricter checking (Git compatibility; accepted for scripts like t3800).
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Clone, Copy)]
enum ExtraHeaderPolicy {
    Error,
    Warn,
    Ignore,
}

fn parse_extra_header_policy(cfg: &ConfigSet) -> ExtraHeaderPolicy {
    cfg.get("fsck.extraheaderentry")
        .map(|v| match v.to_lowercase().as_str() {
            "error" => ExtraHeaderPolicy::Error,
            "ignore" => ExtraHeaderPolicy::Ignore,
            _ => ExtraHeaderPolicy::Warn,
        })
        .unwrap_or(ExtraHeaderPolicy::Warn)
}

fn check_tag_trailer_fsck(
    oid: &ObjectId,
    data: &[u8],
    policy: ExtraHeaderPolicy,
    strict: bool,
    is_reachable: bool,
    issues: &mut Vec<Issue>,
) {
    match fsck_tag_mktag_trailer(data) {
        Ok(()) => {}
        Err(e) if e.id == "extraHeaderEntry" => match policy {
            ExtraHeaderPolicy::Ignore => {}
            ExtraHeaderPolicy::Warn => {
                if strict && is_reachable {
                    issues.push(Issue::BadObject {
                        oid: *oid,
                        kind: ObjectKind::Tag,
                        reason: format!("{}: {}", e.id, e.detail),
                    });
                } else {
                    issues.push(Issue::Warning(format!(
                        "in tag {}: {}: {}",
                        oid.to_hex(),
                        e.id,
                        e.detail
                    )));
                }
            }
            ExtraHeaderPolicy::Error => {
                issues.push(Issue::BadObject {
                    oid: *oid,
                    kind: ObjectKind::Tag,
                    reason: format!("{}: {}", e.id, e.detail),
                });
            }
        },
        Err(e) => {
            issues.push(Issue::BadObject {
                oid: *oid,
                kind: ObjectKind::Tag,
                reason: format!("{}: {}", e.id, e.detail),
            });
        }
    }
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
    /// Reflog references an object that is not present and not a promisor object.
    InvalidReflog { refname: String, oid: ObjectId },
    /// Loose file path does not match the OID of the stored bytes.
    HashPathMismatch { real_oid_hex: String, path: String },
    /// Raw `error:` lines matching Git's `fsck` / `read_loose_object` diagnostics.
    FsckMessage(String),
    /// Non-fatal `warning:` line (e.g. `fsck.extraHeaderEntry=warn` on tag objects).
    Warning(String),
}

/// Run `grit fsck`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;
    // In linked worktrees, object storage lives in the common gitdir, not
    // under `.git/worktrees/<name>/objects`.
    let objects_dir = repo.odb.objects_dir().to_path_buf();
    validate_alternate_paths_exist(&objects_dir)?;
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
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let extra_header_policy = parse_extra_header_policy(&config);
    let promisor_active = repo_treats_promisor_packs(&repo.git_dir, &config);
    // Objects listed in promisor pack indexes — traversal stops here (Git does not recurse
    // past promisor-pack objects).
    let promisor_pack_oids = promisor_pack_object_ids(&objects_dir);
    // Full promisor closure: pack objects plus referenced OIDs (parents, trees, tag targets,
    // etc.). Used for reflog validation and for treating missing loose objects as promised.
    let promisor_expanded: HashSet<ObjectId> = if promisor_active {
        promisor_expanded_object_ids(&repo).context("failed to compute promisor object set")?
    } else {
        HashSet::new()
    };
    let packed_ids = collect_packed_ids(&objects_dir)?;
    let shallow_boundaries = load_shallow_boundaries(repo.git_dir.as_path());

    let (reachable, walked_kinds) = walk_reachable(
        &repo,
        &odb,
        &objects_dir,
        &packed_ids,
        connectivity_only,
        promisor_active,
        &promisor_pack_oids,
        &promisor_expanded,
        &shallow_boundaries,
        extra_header_policy,
        args.strict,
        &mut issues,
    )?;

    check_reflog_entries(
        &repo.git_dir,
        &odb,
        &packed_ids,
        promisor_active,
        &promisor_expanded,
        &mut issues,
    )?;

    // 2. Enumerate all known objects (loose + packed).
    let all_objects = enumerate_all_objects(&odb, &objects_dir)?;
    let loose_pairs = scan_loose_objects(&objects_dir)?;
    let loose_oids: HashSet<ObjectId> = loose_pairs.iter().map(|(o, _)| *o).collect();

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
        let git_dir = repo.git_dir.as_path();
        for (oid, path) in &loose_pairs {
            // Objects reached in the walk were already content-checked; skip to avoid duplicate
            // diagnostics (e.g. t4212 expects one `git fsck` line per bad commit).
            if walked_kinds.contains(oid) {
                continue;
            }
            validate_loose_object_file(
                git_dir,
                path,
                oid,
                extra_header_policy,
                args.strict,
                reachable.contains(oid),
                &mut issues,
            );
        }
        for oid in &all_objects {
            if loose_oids.contains(oid) {
                continue;
            }
            if walked_kinds.contains(oid) {
                continue;
            }
            validate_object(
                &odb,
                oid,
                extra_header_policy,
                args.strict,
                reachable.contains(oid),
                &mut issues,
            );
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
                // Git: on tag objects, `badTagName` / `missingTaggerEntry` are reported as
                // `warning in tag` and do not fail fsck. `badEmail` still prints `error in tag`
                // but dangling/unreachable tags with bad email still yield exit 0 (t3800).
                let tag_soft = *kind == ObjectKind::Tag
                    && (reason.starts_with("badTagName:")
                        || reason.starts_with("missingTaggerEntry:")
                        || reason.starts_with("badEmail:"));
                if tag_soft {
                    let prefix = if reason.starts_with("badEmail:") {
                        "error"
                    } else {
                        "warning"
                    };
                    eprintln!(
                        "{prefix} in {} {}{}: {}",
                        kind.as_str(),
                        oid.to_hex(),
                        name_suffix,
                        reason
                    );
                } else {
                    eprintln!(
                        "error in {} {}{}: {}",
                        kind.as_str(),
                        oid.to_hex(),
                        name_suffix,
                        reason
                    );
                    has_errors = true;
                }
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
            Issue::InvalidReflog { refname, oid } => {
                eprintln!("error: {}: invalid reflog entry {}", refname, oid.to_hex());
                has_errors = true;
            }
            Issue::HashPathMismatch { real_oid_hex, path } => {
                eprintln!("error: {real_oid_hex}: hash-path mismatch, found at: {path}");
                has_errors = true;
            }
            Issue::FsckMessage(msg) => {
                eprintln!("error: {msg}");
                has_errors = true;
            }
            Issue::Warning(msg) => {
                eprintln!("warning {msg}");
            }
        }
    }

    if has_errors {
        // Match Git: repository problems yield exit code 2 (not 1). Use `ExplicitExit` so POSIX
        // shells running `git fsck` under `set -e` do not treat exit 2 as a hard failure mid-pipeline.
        return Err(anyhow::Error::new(ExplicitExit {
            code: 2,
            message: String::new(),
        }));
    }

    Ok(())
}

/// Label for `missing <kind>` diagnostics (Git uses `blob` when the parent is a tree).
fn missing_object_kind_for_referrer(odb: &Odb, referrer: Option<ObjectId>) -> &'static str {
    let Some(ref_oid) = referrer else {
        return "object";
    };
    let Ok(obj) = odb.read(&ref_oid) else {
        return "object";
    };
    match obj.kind {
        ObjectKind::Tree => "blob",
        ObjectKind::Commit => "tree",
        ObjectKind::Tag | ObjectKind::Blob => "object",
    }
}

/// Walk all reachable objects from refs and HEAD.
/// Returns (reachable set, set of OIDs whose content was validated).
fn walk_reachable(
    repo: &Repository,
    odb: &Odb,
    objects_dir: &Path,
    packed_ids: &HashSet<ObjectId>,
    connectivity_only: bool,
    promisor_active: bool,
    promisor_pack_oids: &HashSet<ObjectId>,
    promisor_expanded: &HashSet<ObjectId>,
    shallow_boundaries: &HashSet<ObjectId>,
    extra_header_policy: ExtraHeaderPolicy,
    strict: bool,
    issues: &mut Vec<Issue>,
) -> Result<(HashSet<ObjectId>, HashSet<ObjectId>)> {
    let _ = objects_dir;
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

    // Seed commit OIDs mentioned in reflogs so we walk trees/blobs reachable only via history
    // (matches `git fsck`: missing blobs in reflog-only commits are reported as `missing blob`).
    let z = zero_oid();
    if let Ok(refnames) = list_reflog_refs(&repo.git_dir) {
        for refname in refnames {
            if let Ok(entries) = read_reflog(&repo.git_dir, &refname) {
                for e in entries {
                    if e.old_oid != z {
                        queue.push_back((e.old_oid, None));
                    }
                    if e.new_oid != z {
                        queue.push_back((e.new_oid, None));
                    }
                }
            }
        }
    }

    // BFS walk.
    while let Some((oid, referrer)) = queue.pop_front() {
        if !reachable.insert(oid) {
            continue;
        }

        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => {
                if packed_ids.contains(&oid) {
                    continue;
                }
                if promisor_active && promisor_expanded.contains(&oid) {
                    continue;
                }
                let ref_oid = referrer.unwrap_or(oid);
                let kind = missing_object_kind_for_referrer(odb, referrer);
                issues.push(Issue::Missing {
                    oid,
                    kind,
                    referenced_by: ref_oid,
                });
                continue;
            }
        };

        // Validate object content if not connectivity-only.
        if !connectivity_only {
            validated.insert(oid);
            validate_object_data(
                &oid,
                &obj.kind,
                &obj.data,
                extra_header_policy,
                strict,
                true,
                issues,
            );
        }

        // Objects stored in promisor packs stop traversal (do not walk into parents/trees).
        if promisor_active && promisor_pack_oids.contains(&oid) {
            continue;
        }

        match obj.kind {
            ObjectKind::Commit => {
                if let Ok(commit) = parse_commit(&obj.data) {
                    queue.push_back((commit.tree, Some(oid)));
                    // Shallow clones: do not require parent objects at boundary commits.
                    if !shallow_boundaries.contains(&oid) {
                        for parent in commit.parents {
                            queue.push_back((parent, Some(oid)));
                        }
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
                if let Some(target) = tag_object_line_oid(&obj.data) {
                    queue.push_back((target, Some(oid)));
                }
            }
            ObjectKind::Blob => {}
        }
    }

    Ok((reachable, validated))
}

fn check_reflog_entries(
    git_dir: &Path,
    odb: &Odb,
    packed_ids: &HashSet<ObjectId>,
    promisor_active: bool,
    promisor_expanded: &HashSet<ObjectId>,
    issues: &mut Vec<Issue>,
) -> Result<()> {
    if grit_lib::reftable::is_reftable_repo(git_dir) {
        return Ok(());
    }
    let logs_root = git_dir.join("logs");
    if !logs_root.is_dir() {
        return Ok(());
    }
    check_reflog_dir(
        &logs_root,
        &logs_root,
        odb,
        packed_ids,
        promisor_active,
        promisor_expanded,
        issues,
    )?;
    Ok(())
}

fn check_reflog_dir(
    base: &Path,
    dir: &Path,
    odb: &Odb,
    packed_ids: &HashSet<ObjectId>,
    promisor_active: bool,
    promisor_expanded: &HashSet<ObjectId>,
    issues: &mut Vec<Issue>,
) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| anyhow::anyhow!(e))? {
        let entry = entry.map_err(|e| anyhow::anyhow!(e))?;
        let path = entry.path();
        if path.is_dir() {
            check_reflog_dir(
                base,
                &path,
                odb,
                packed_ids,
                promisor_active,
                promisor_expanded,
                issues,
            )?;
        } else if path.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let refname = reflog_relative_to_refname(rel);
            let content = fs::read_to_string(&path).map_err(|e| anyhow::anyhow!(e))?;
            for line in content.lines() {
                if let Some((old_oid, new_oid)) = parse_reflog_line_oids(line) {
                    for oid in [old_oid, new_oid] {
                        if oid == zero_oid() {
                            continue;
                        }
                        if odb.read(&oid).is_ok() || packed_ids.contains(&oid) {
                            continue;
                        }
                        if promisor_active && promisor_expanded.contains(&oid) {
                            continue;
                        }
                        issues.push(Issue::InvalidReflog {
                            refname: refname.clone(),
                            oid,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn reflog_relative_to_refname(rel: &Path) -> String {
    let s = rel.to_string_lossy().replace('\\', "/");
    if s == "HEAD" {
        "HEAD".to_string()
    } else {
        s
    }
}

fn parse_reflog_line_oids(line: &str) -> Option<(ObjectId, ObjectId)> {
    let before_tab = line.split('\t').next()?;
    if before_tab.len() < 83 {
        return None;
    }
    let old_hex = &before_tab[..40];
    let new_hex = &before_tab[41..81];
    Some((old_hex.parse().ok()?, new_hex.parse().ok()?))
}

fn loose_path_display_for_fsck(git_dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(git_dir).unwrap_or(path);
    format!("./{}", rel.display().to_string().replace('\\', "/"))
}

fn git_style_inflate_message(zlib_detail: &str) -> String {
    let lower = zlib_detail.to_ascii_lowercase();
    if lower.contains("dictionary") {
        "inflate: needs dictionary".to_owned()
    } else if lower.contains("incorrect header")
        || lower.contains("invalid stored block")
        || lower.contains("invalid code")
        || lower.contains("corrupt deflate")
    {
        "inflate: data stream error (incorrect header check)".to_owned()
    } else {
        format!("inflate: {zlib_detail}")
    }
}

/// Validate a loose object file, including Git's hash-vs-path check.
fn validate_loose_object_file(
    git_dir: &Path,
    path: &Path,
    oid: &ObjectId,
    extra_header_policy: ExtraHeaderPolicy,
    strict: bool,
    is_reachable: bool,
    issues: &mut Vec<Issue>,
) {
    let display_path = loose_path_display_for_fsck(git_dir, path);
    match Odb::read_loose_verify_oid(path, oid) {
        Ok(obj) => validate_object_data(
            oid,
            &obj.kind,
            &obj.data,
            extra_header_policy,
            strict,
            is_reachable,
            issues,
        ),
        Err(LibError::LooseHashMismatch { path: _, real_oid }) => {
            issues.push(Issue::HashPathMismatch {
                real_oid_hex: real_oid,
                path: display_path,
            });
        }
        Err(LibError::Zlib(msg)) => {
            let inflate = git_style_inflate_message(&msg);
            issues.push(Issue::FsckMessage(inflate));
            issues.push(Issue::FsckMessage(format!(
                "unable to unpack header of {display_path}"
            )));
            issues.push(Issue::FsckMessage(format!(
                "{}: object corrupt or missing: {display_path}",
                oid.to_hex()
            )));
        }
        Err(e) => {
            issues.push(Issue::FsckMessage(e.to_string()));
        }
    }
}

/// Validate an object's content can be parsed correctly.
fn validate_object(
    odb: &Odb,
    oid: &ObjectId,
    extra_header_policy: ExtraHeaderPolicy,
    strict: bool,
    is_reachable: bool,
    issues: &mut Vec<Issue>,
) {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => return, // can't read loose — might be packed only
    };
    validate_object_data(
        oid,
        &obj.kind,
        &obj.data,
        extra_header_policy,
        strict,
        is_reachable,
        issues,
    );
}

/// Validate the parsed content of an object.
fn validate_object_data(
    oid: &ObjectId,
    kind: &ObjectKind,
    data: &[u8],
    extra_header_policy: ExtraHeaderPolicy,
    strict: bool,
    is_reachable: bool,
    issues: &mut Vec<Issue>,
) {
    match kind {
        ObjectKind::Commit => {
            if let Err(msg) = fsck_commit_idents(data) {
                issues.push(Issue::BadObject {
                    oid: *oid,
                    kind: *kind,
                    reason: msg,
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
            if let Err(e) = fsck_object(ObjectKind::Tag, data) {
                let mut reason = e.report_line();
                if e.id == "badType" && e.detail == "invalid 'type' value" {
                    if let Some(typ) = grit_lib::objects::tag_header_field(data, b"type ") {
                        reason = format!("unknown tag type '{typ}'");
                    }
                }
                issues.push(Issue::BadObject {
                    oid: *oid,
                    kind: *kind,
                    reason,
                });
            } else {
                check_tag_trailer_fsck(
                    oid,
                    data,
                    extra_header_policy,
                    strict,
                    is_reachable,
                    issues,
                );
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
                if let Some(target) = tag_object_line_oid(&obj.data) {
                    referenced.insert(target);
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

/// Fail when `objects/info/alternates` lists paths that no longer exist.
///
/// Git reports `unable to normalize alternate object path` and exits non-zero;
/// this catches clones that still point at a removed `--reference` repository.
fn validate_alternate_paths_exist(objects_dir: &Path) -> Result<()> {
    let alt_file = objects_dir.join("info/alternates");
    let Ok(content) = fs::read_to_string(&alt_file) else {
        return Ok(());
    };
    let base = fs::canonicalize(objects_dir).unwrap_or_else(|_| objects_dir.to_path_buf());
    let mut bad = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let path = if Path::new(line).is_absolute() {
            PathBuf::from(line)
        } else {
            base.join(line)
        };
        if !path.exists() {
            eprintln!("error: unable to normalize alternate object path: {}", line);
            bad = true;
        }
    }
    if bad {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 2,
            message: String::new(),
        }));
    }
    Ok(())
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
