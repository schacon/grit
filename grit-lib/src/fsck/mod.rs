//! Repository-wide integrity checks (object store, connectivity, refs, MIDX, commit-graph).
//!
//! Validates loose and packed objects (in parallel when worthwhile), walks reachability from
//! [`reachability_roots`], optionally reports dangling objects, and runs ref/MIDX/graph checks.
//! Progress is reported through [`ProgressSink`]; the library never prints.

mod roots;

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::ops::ControlFlow;
use std::path::PathBuf;

use crate::commit_graph_file::CommitGraphChain;
use crate::config::ConfigSet;
use crate::error::{Error, Result};
use crate::fsck_standalone::{fsck_object, FsckError, FsckObjectOptions};
use crate::gitmodules::{DotFsckIssue, DotFsckTracker};
use crate::index::MODE_GITLINK;
use crate::midx::verify_midx;
use crate::objects::{parse_commit, parse_tag, parse_tree, ObjectId, ObjectKind};
use crate::odb::Odb;
use crate::pack::verify_pack_and_collect;
use crate::parallel::{try_par_map, ParallelOp, ParallelOptions};
use crate::progress::{Cancel, ProgressPhase, ProgressSink};
use crate::refs_fsck::{refs_fsck, RefsFsckIssue};
use crate::repo::Repository;

pub use roots::reachability_roots;

/// Controls which fsck phases run and how strictly diagnostics are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FsckOptions {
    /// When true, skip full object-store validation and only check connectivity from roots.
    pub connectivity_only: bool,
    /// When true, compute [`FsckReport::dangling`] after the reachability walk.
    pub check_dangling: bool,
    /// Treat ref-fsck warnings as errors (Git `--strict` for references).
    pub strict: bool,
}

impl Default for FsckOptions {
    fn default() -> Self {
        Self {
            connectivity_only: false,
            check_dangling: true,
            strict: false,
        }
    }
}

impl FsckOptions {
    /// Full object validation, dangling detection, optional strict refs.
    #[must_use]
    pub fn full_repository(strict: bool) -> Self {
        Self {
            connectivity_only: false,
            check_dangling: true,
            strict,
        }
    }

    /// Connectivity and refs only (no store-wide object fsck, no dangling list).
    #[must_use]
    pub fn connectivity_and_refs(strict: bool) -> Self {
        Self {
            connectivity_only: true,
            check_dangling: false,
            strict,
        }
    }

    /// Copy with [`Self::check_dangling`] replaced.
    #[must_use]
    pub fn with_check_dangling(mut self, check_dangling: bool) -> Self {
        self.check_dangling = check_dangling;
        self
    }
}

/// One repository fsck finding with a stable, typed shape.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FsckIssue {
    /// Loose object file could not be read or decompressed.
    CorruptLoose {
        /// Expected object id from the path.
        oid: ObjectId,
        /// Path to the loose object file.
        path: PathBuf,
    },
    /// Loose file content hashes to a different id than its path.
    HashMismatch {
        /// Id encoded in the path.
        oid: ObjectId,
        /// Loose object path.
        path: PathBuf,
    },
    /// Standalone object buffer fsck failed.
    BadObject {
        /// Object id.
        oid: ObjectId,
        /// Parsed fsck error (Git-compatible msg id in [`FsckError::id`]).
        error: FsckError,
    },
    /// Pack trailing checksum or structure check failed.
    PackChecksum {
        /// Path to the `.pack` file.
        pack: PathBuf,
    },
    /// Pack checksum embedded in the `.idx` does not match the pack trailer.
    IdxMismatch {
        /// Path to the `.pack` file (index is alongside with `.idx`).
        pack: PathBuf,
    },
    /// Reachability walk found a missing object.
    MissingObject {
        /// Missing object id.
        oid: ObjectId,
        /// Object id that referenced the missing object.
        referenced_by: ObjectId,
    },
    /// Tree entry points at an object of the wrong kind (or an invalid link).
    BrokenLink {
        /// Tree object containing the bad entry.
        tree: ObjectId,
        /// Entry path component (UTF-8 lossy).
        name: String,
        /// Mode from the tree entry.
        mode: u32,
        /// Target oid from the tree entry.
        target: ObjectId,
    },
    /// Reference database problem (symref, packed-refs, etc.).
    BadRef(RefsFsckIssue),
    /// Multi-pack-index verification failed.
    MidxInvalid {
        /// Human-readable detail strings from [`verify_midx`].
        details: Vec<String>,
    },
    /// Commit-graph file is present but invalid.
    CommitGraphInvalid {
        /// Error detail.
        detail: String,
    },
}

/// Severity for reporting and exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsckSeverity {
    /// Should fail the overall fsck run.
    Error,
    /// Acceptable depending on `fsck.<msg-id>` policy.
    Warning,
}

/// Aggregate fsck outcome.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FsckReport {
    /// Loose and packed objects examined during store validation.
    pub objects_checked: usize,
    /// All recorded findings in discovery order.
    pub issues: Vec<FsckIssue>,
    /// Objects present in the object database but not reachable from [`reachability_roots`].
    pub dangling: Vec<(ObjectKind, ObjectId)>,
}

impl FsckReport {
    /// True when no error-severity issues are present under `options` and `config`.
    #[must_use]
    pub fn ok(&self, config: &ConfigSet, options: &FsckOptions) -> bool {
        !self
            .issues
            .iter()
            .any(|issue| issue_severity(issue, config, options.strict) == FsckSeverity::Error)
    }
}

/// CamelCase Git fsck msg-id for `issue`.
#[must_use]
pub fn issue_msg_id(issue: &FsckIssue) -> &'static str {
    match issue {
        FsckIssue::CorruptLoose { .. } => "corruptObject",
        FsckIssue::HashMismatch { .. } => "hasSha1",
        FsckIssue::BadObject { error, .. } => error.id,
        FsckIssue::PackChecksum { .. } => "badPackChecksum",
        FsckIssue::IdxMismatch { .. } => "badPackIndex",
        FsckIssue::MissingObject { .. } => "missingObject",
        FsckIssue::BrokenLink { .. } => "brokenLink",
        FsckIssue::BadRef(r) => r.msg_id,
        FsckIssue::MidxInvalid { .. } => "badMidx",
        FsckIssue::CommitGraphInvalid { .. } => "badCommitGraph",
    }
}

/// Resolve severity using `fsck.<msg-id>` and `strict`.
#[must_use]
pub fn issue_severity(issue: &FsckIssue, config: &ConfigSet, strict: bool) -> FsckSeverity {
    if let FsckIssue::BadRef(r) = issue {
        return match r.severity {
            crate::refs_fsck::RefsFsckSeverity::Error => FsckSeverity::Error,
            crate::refs_fsck::RefsFsckSeverity::Warning if strict => FsckSeverity::Error,
            crate::refs_fsck::RefsFsckSeverity::Warning => FsckSeverity::Warning,
        };
    }
    let msg_id = issue_msg_id(issue);
    let key = format!("fsck.{msg_id}");
    match config.get(&key).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("ignore") => FsckSeverity::Warning,
        Some("warn") => FsckSeverity::Warning,
        Some("error") => FsckSeverity::Error,
        _ if strict => FsckSeverity::Error,
        _ => FsckSeverity::Error,
    }
}

/// Format one issue like Git fsck (`level: path: msgId: detail`).
#[must_use]
pub fn format_fsck_line(issue: &FsckIssue, config: &ConfigSet, strict: bool) -> String {
    let level = match issue_severity(issue, config, strict) {
        FsckSeverity::Error => "error",
        FsckSeverity::Warning => "warning",
    };
    let (path, detail) = issue_display_paths(issue);
    format!("{level}: {path}: {}: {detail}", issue_msg_id(issue))
}

fn issue_display_paths(issue: &FsckIssue) -> (String, String) {
    match issue {
        FsckIssue::CorruptLoose { oid, path } => {
            (oid.to_hex(), format!("could not read {}", path.display()))
        }
        FsckIssue::HashMismatch { oid, path } => (
            oid.to_hex(),
            format!("loose hash mismatch at {}", path.display()),
        ),
        FsckIssue::BadObject { oid, error } => (oid.to_hex(), error.detail.clone()),
        FsckIssue::PackChecksum { pack } => (
            pack.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("pack")
                .to_owned(),
            format!("pack checksum invalid for {}", pack.display()),
        ),
        FsckIssue::IdxMismatch { pack } => (
            pack.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("pack")
                .to_owned(),
            format!("index checksum mismatch for {}", pack.display()),
        ),
        FsckIssue::MissingObject { oid, referenced_by } => (
            oid.to_hex(),
            format!("referenced by {}", referenced_by.to_hex()),
        ),
        FsckIssue::BrokenLink {
            tree,
            name,
            mode,
            target,
        } => (
            tree.to_hex(),
            format!("entry {name} mode {mode:o} -> {}", target.to_hex()),
        ),
        FsckIssue::BadRef(r) => (r.path.clone(), r.detail.clone()),
        FsckIssue::MidxInvalid { details } => ("multi-pack-index".to_owned(), details.join("; ")),
        FsckIssue::CommitGraphInvalid { detail } => ("commit-graph".to_owned(), detail.clone()),
    }
}

/// Run repository fsck with optional progress and cancellation.
///
/// # Errors
///
/// Propagates object-store, ref, or index I/O failures that abort the run.
pub fn fsck(
    repo: &Repository,
    options: &FsckOptions,
    progress: &mut dyn ProgressSink,
    cancel: &dyn Cancel,
) -> Result<FsckReport> {
    let config = repo.config()?;
    let odb = repo.odb();
    if !options.connectivity_only {
        odb.invalidate_packs();
    }

    let mut issues = Vec::new();
    let mut objects_checked = 0usize;

    if !options.connectivity_only {
        objects_checked +=
            verify_loose_objects(repo, &config, options.strict, &mut issues, progress, cancel)?;
        objects_checked += verify_pack_objects(repo, &mut issues, progress, cancel)?;
    }

    verify_midx_and_commit_graph(repo, &mut issues)?;

    for rissue in refs_fsck(repo, odb, &config, options.strict)? {
        if should_emit(&config, options.strict, rissue.msg_id) {
            issues.push(FsckIssue::BadRef(rissue));
        }
    }

    let roots = reachability_roots(repo)?;
    let (reachable, connectivity_issues) =
        walk_connectivity(repo, &roots, &config, options.strict, progress, cancel)?;
    issues.extend(connectivity_issues);

    fsck_index_missing_blobs(repo, &config, options.strict, &mut issues)?;

    let mut dangling = Vec::new();
    if options.check_dangling {
        let mut all_objects: HashSet<ObjectId> = HashSet::new();
        odb.for_each_object(&mut |oid| {
            all_objects.insert(*oid);
            ControlFlow::Continue(())
        })?;
        for oid in all_objects {
            if reachable.contains(&oid) {
                continue;
            }
            if let Ok(obj) = odb.read(&oid) {
                dangling.push((obj.kind, oid));
            }
        }
        dangling.sort_by_key(|a| a.1);
    }

    Ok(FsckReport {
        objects_checked,
        issues,
        dangling,
    })
}

fn should_emit(config: &ConfigSet, strict: bool, msg_id: &str) -> bool {
    let key = format!("fsck.{msg_id}");
    let level = config.get(&key).map(|s| s.to_ascii_lowercase());
    !matches!(level.as_deref(), Some("ignore") if !strict)
}

fn push_bad_object(
    issues: &mut Vec<FsckIssue>,
    config: &ConfigSet,
    strict: bool,
    oid: ObjectId,
    err: FsckError,
) {
    if should_emit(config, strict, err.id) {
        issues.push(FsckIssue::BadObject { oid, error: err });
    }
}

enum LooseCheckOutcome {
    Clean,
    Issue(FsckIssue),
}

fn verify_loose_objects(
    repo: &Repository,
    config: &ConfigSet,
    strict: bool,
    issues: &mut Vec<FsckIssue>,
    progress: &mut dyn ProgressSink,
    cancel: &dyn Cancel,
) -> Result<usize> {
    let odb = repo.odb();
    let loose = odb.enumerate_local_loose_objects()?;
    let total = loose.len();
    progress.start(ProgressPhase::IndexingObjects, Some(total as u64));
    let par_opts: ParallelOptions = repo.thread_policy().resolve(ParallelOp::IndexPack, config);
    let fsck_opts = FsckObjectOptions::new(odb.hash_algo());
    let mut dot = DotFsckTracker::default();

    if cancel.is_cancelled() {
        return Err(Error::Message("cancelled".into()));
    }
    let results = match try_par_map::<_, _, String, _>(&loose, par_opts, total, |(oid, path)| {
        let read = Odb::read_loose_verify_oid(path, oid);
        match read {
            Err(Error::LooseHashMismatch { .. }) => {
                Ok(LooseCheckOutcome::Issue(FsckIssue::HashMismatch {
                    oid: *oid,
                    path: path.clone(),
                }))
            }
            Err(_) => Ok(LooseCheckOutcome::Issue(FsckIssue::CorruptLoose {
                oid: *oid,
                path: path.clone(),
            })),
            Ok(obj) => {
                if let Err(err) = fsck_object(obj.kind, &obj.data, fsck_opts) {
                    return Ok(LooseCheckOutcome::Issue(FsckIssue::BadObject {
                        oid: *oid,
                        error: err,
                    }));
                }
                Ok(LooseCheckOutcome::Clean)
            }
        }
    }) {
        Ok(v) => v,
        Err(e) => return Err(Error::Message(format!("parallel fsck: {e}"))),
    };

    let mut checked = 0usize;
    for (item, outcome) in loose.iter().zip(results) {
        checked += 1;
        if checked.is_multiple_of(128) {
            progress.set(checked as u64);
            if cancel.is_cancelled() {
                return Err(Error::Message("cancelled".into()));
            }
        }
        match outcome {
            LooseCheckOutcome::Clean => {
                if let Ok(obj) = Odb::read_loose_verify_oid(&item.1, &item.0) {
                    for dot_issue in dot.on_object(&item.0, obj.kind, &obj.data) {
                        push_dot_issue(issues, config, strict, dot_issue);
                    }
                }
            }
            LooseCheckOutcome::Issue(issue) => match &issue {
                FsckIssue::BadObject { oid, error } => {
                    push_bad_object(issues, config, strict, *oid, error.clone());
                }
                other => {
                    if should_emit(config, strict, issue_msg_id(other)) {
                        issues.push(other.clone());
                    }
                }
            },
        }
    }
    for pending in dot.finish_pending(odb)? {
        push_dot_issue(issues, config, strict, pending);
    }
    progress.finish();
    Ok(checked)
}

fn push_dot_issue(
    issues: &mut Vec<FsckIssue>,
    config: &ConfigSet,
    strict: bool,
    dot: DotFsckIssue,
) {
    let (oid, id, detail) = match dot {
        DotFsckIssue::TreeSymlink {
            tree_oid,
            id,
            detail,
        } => (tree_oid, id, detail.to_owned()),
        DotFsckIssue::NonBlobDotFile {
            oid, id, detail, ..
        } => (oid, id, detail.to_owned()),
        DotFsckIssue::BlobGitmodules {
            blob_oid,
            id,
            detail,
        } => (blob_oid, id, detail),
        DotFsckIssue::BlobGitattributes {
            blob_oid,
            id,
            detail,
        } => (blob_oid, id, detail.to_owned()),
    };
    if should_emit(config, strict, id) {
        issues.push(FsckIssue::BadObject {
            oid,
            error: FsckError::new(id, detail),
        });
    }
}

fn verify_pack_objects(
    repo: &Repository,
    issues: &mut Vec<FsckIssue>,
    progress: &mut dyn ProgressSink,
    cancel: &dyn Cancel,
) -> Result<usize> {
    let Some(objects_dir) = repo.odb().files_objects_dir() else {
        return Ok(0);
    };
    let pack_dir = objects_dir.join("pack");
    if !pack_dir.is_dir() {
        return Ok(0);
    }
    let packs: Vec<PathBuf> = fs::read_dir(&pack_dir)
        .map_err(Error::Io)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "idx"))
        .collect();
    progress.start(ProgressPhase::IndexingObjects, Some(packs.len() as u64));
    let mut checked = 0usize;
    for (i, idx_path) in packs.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(Error::Message("cancelled".into()));
        }
        let pack_path = idx_path.with_extension("pack");
        match verify_pack_and_collect(idx_path) {
            Ok(records) => {
                checked += records.len();
            }
            Err(Error::CorruptObject(msg)) => {
                let issue = if msg.contains("checksum") && msg.contains("index") {
                    FsckIssue::IdxMismatch { pack: pack_path }
                } else {
                    FsckIssue::PackChecksum { pack: pack_path }
                };
                issues.push(issue);
            }
            Err(e) => return Err(e),
        }
        progress.set((i + 1) as u64);
    }
    progress.finish();
    Ok(checked)
}

fn verify_midx_and_commit_graph(repo: &Repository, issues: &mut Vec<FsckIssue>) -> Result<()> {
    let odb = repo.odb();
    if let Some(objects_dir) = odb.files_objects_dir() {
        let pack_dir = objects_dir.join("pack");
        if pack_dir.join("multi-pack-index").is_file() {
            if let Err(errs) = verify_midx(&pack_dir) {
                issues.push(FsckIssue::MidxInvalid { details: errs });
            }
        }
        match CommitGraphChain::try_load(objects_dir) {
            Ok(None) => {}
            Ok(Some(_)) => {}
            Err(e) => issues.push(FsckIssue::CommitGraphInvalid {
                detail: e.to_string(),
            }),
        }
    }
    Ok(())
}

fn walk_connectivity(
    repo: &Repository,
    roots: &[ObjectId],
    config: &ConfigSet,
    strict: bool,
    progress: &mut dyn ProgressSink,
    cancel: &dyn Cancel,
) -> Result<(HashSet<ObjectId>, Vec<FsckIssue>)> {
    let odb = repo.odb();
    let fsck_opts = FsckObjectOptions::new(odb.hash_algo());
    let mut issues = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut queue: VecDeque<(ObjectId, ObjectId)> = VecDeque::new();
    for &root in roots {
        if seen.insert(root) {
            queue.push_back((root, ObjectId::zero()));
        }
    }
    progress.start(ProgressPhase::CopyingObjects, None);
    let mut visited = 0u64;
    while let Some((oid, parent)) = queue.pop_front() {
        if cancel.is_cancelled() {
            return Err(Error::Message("cancelled".into()));
        }
        visited += 1;
        if visited.is_multiple_of(256) {
            progress.set(visited);
        }
        let obj = match odb.read(&oid) {
            Ok(o) => o,
            Err(_) => {
                if should_emit(config, strict, "missingObject") {
                    issues.push(FsckIssue::MissingObject {
                        oid,
                        referenced_by: parent,
                    });
                }
                continue;
            }
        };
        if let Err(err) = fsck_object(obj.kind, &obj.data, fsck_opts) {
            push_bad_object(&mut issues, config, strict, oid, err);
        }
        match obj.kind {
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data)?;
                for p in commit.parents {
                    enqueue(&mut queue, &mut seen, p, oid);
                }
                enqueue(&mut queue, &mut seen, commit.tree, oid);
            }
            ObjectKind::Tree => {
                for entry in parse_tree(&obj.data)? {
                    if entry.mode == MODE_GITLINK {
                        if odb.read(&entry.oid).is_err()
                            && should_emit(config, strict, "missingObject")
                        {
                            issues.push(FsckIssue::MissingObject {
                                oid: entry.oid,
                                referenced_by: oid,
                            });
                        }
                        continue;
                    }
                    if let Some(expected) = kind_for_tree_mode(entry.mode) {
                        match odb.read(&entry.oid) {
                            Err(_) => {
                                if should_emit(config, strict, "missingObject") {
                                    issues.push(FsckIssue::MissingObject {
                                        oid: entry.oid,
                                        referenced_by: oid,
                                    });
                                }
                            }
                            Ok(child) if child.kind != expected => {
                                if should_emit(config, strict, "brokenLink") {
                                    issues.push(FsckIssue::BrokenLink {
                                        tree: oid,
                                        name: String::from_utf8_lossy(&entry.name).into_owned(),
                                        mode: entry.mode,
                                        target: entry.oid,
                                    });
                                }
                            }
                            Ok(_) => enqueue(&mut queue, &mut seen, entry.oid, oid),
                        }
                    } else if should_emit(config, strict, "brokenLink") {
                        issues.push(FsckIssue::BrokenLink {
                            tree: oid,
                            name: String::from_utf8_lossy(&entry.name).into_owned(),
                            mode: entry.mode,
                            target: entry.oid,
                        });
                    }
                }
            }
            ObjectKind::Tag => {
                let tag = parse_tag(&obj.data)?;
                enqueue(&mut queue, &mut seen, tag.object, oid);
            }
            ObjectKind::Blob => {}
        }
    }
    progress.finish();
    Ok((seen, issues))
}

fn enqueue(
    queue: &mut VecDeque<(ObjectId, ObjectId)>,
    seen: &mut HashSet<ObjectId>,
    oid: ObjectId,
    parent: ObjectId,
) {
    if seen.insert(oid) {
        queue.push_back((oid, parent));
    }
}

fn kind_for_tree_mode(mode: u32) -> Option<ObjectKind> {
    match mode {
        0o040000 => Some(ObjectKind::Tree),
        0o100644 | 0o100755 | 0o120000 => Some(ObjectKind::Blob),
        0o160000 => Some(ObjectKind::Commit),
        _ => None,
    }
}

fn fsck_index_missing_blobs(
    repo: &Repository,
    config: &ConfigSet,
    strict: bool,
    issues: &mut Vec<FsckIssue>,
) -> Result<()> {
    let index = repo.load_index()?;
    let odb = repo.odb();
    for entry in index.entries() {
        if entry.mode == MODE_GITLINK || entry.oid.is_zero() {
            continue;
        }
        if odb.read(&entry.oid).is_err() && should_emit(config, strict, "missingObject") {
            issues.push(FsckIssue::MissingObject {
                oid: entry.oid,
                referenced_by: entry.oid,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::porcelain::add::stage_for_commit;
    use crate::porcelain::commit::{create_commit, CommitRequest};
    use crate::repo::InitOptions;
    use tempfile::TempDir;

    #[test]
    fn clean_repo_fsck_ok() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path(), &InitOptions::default()).expect("init");
        std::fs::write(repo.work_tree().unwrap().join("f"), b"x").unwrap();
        stage_for_commit(&repo, &mut NullProgress).expect("stage");
        let ident = "T <t@example.com> 1 +0000".to_string();
        create_commit(
            &repo,
            &CommitRequest {
                message: "c".into(),
                author: ident.clone(),
                committer: ident,
                allow_empty: false,
                sign_override: None,
                amend: false,
            },
            &mut NullProgress,
        )
        .expect("commit");
        let config = repo.config().expect("config");
        let report = fsck(
            &repo,
            &FsckOptions::full_repository(true),
            &mut NullProgress,
            &NeverCancel,
        )
        .expect("fsck");
        assert!(report.ok(&config, &FsckOptions::full_repository(true)));
        assert!(report.issues.is_empty());
    }
}
