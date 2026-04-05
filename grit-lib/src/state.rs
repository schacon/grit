//! Repository state machine — HEAD resolution, branch status, and
//! in-progress operation detection.
//!
//! # Overview
//!
//! Git repositories can be in various states beyond just "clean":
//! merging, rebasing, cherry-picking, reverting, bisecting, etc.
//! This module detects those states by checking for sentinel files
//! (e.g. `MERGE_HEAD`, `rebase-merge/`) in the `.git` directory.
//!
//! It also resolves `HEAD` to determine the current branch and commit,
//! and provides working tree / index diff summaries used by `status`,
//! `commit`, and other porcelain commands.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::objects::ObjectId;

/// The current state of HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    /// HEAD points to a branch via a symbolic ref (e.g. `ref: refs/heads/main`).
    Branch {
        /// The full ref name (e.g. `refs/heads/main`).
        refname: String,
        /// The short branch name (e.g. `main`).
        short_name: String,
        /// The commit OID that the branch points to, or `None` if the
        /// branch is unborn (no commits yet).
        oid: Option<ObjectId>,
    },
    /// HEAD is detached — pointing directly at a commit.
    Detached {
        /// The commit OID.
        oid: ObjectId,
    },
    /// HEAD is in an invalid or unreadable state.
    Invalid,
}

impl HeadState {
    /// Return the commit OID if HEAD resolves to one.
    #[must_use]
    pub fn oid(&self) -> Option<&ObjectId> {
        match self {
            Self::Branch { oid, .. } => oid.as_ref(),
            Self::Detached { oid } => Some(oid),
            Self::Invalid => None,
        }
    }

    /// Return the branch name if HEAD is on a branch.
    #[must_use]
    pub fn branch_name(&self) -> Option<&str> {
        match self {
            Self::Branch { short_name, .. } => Some(short_name),
            _ => None,
        }
    }

    /// Whether HEAD is on an unborn branch (no commits yet).
    #[must_use]
    pub fn is_unborn(&self) -> bool {
        matches!(self, Self::Branch { oid: None, .. })
    }

    /// Whether HEAD is detached.
    #[must_use]
    pub fn is_detached(&self) -> bool {
        matches!(self, Self::Detached { .. })
    }
}

/// An in-progress operation that the repository is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgressOperation {
    /// A merge is in progress (`MERGE_HEAD` exists).
    Merge,
    /// An interactive rebase is in progress (`rebase-merge/` exists).
    RebaseInteractive,
    /// A non-interactive rebase is in progress (`rebase-apply/` exists).
    Rebase,
    /// A cherry-pick is in progress (`CHERRY_PICK_HEAD` exists).
    CherryPick,
    /// A revert is in progress (`REVERT_HEAD` exists).
    Revert,
    /// A bisect is in progress (`BISECT_LOG` exists).
    Bisect,
    /// An `am` (apply mailbox) is in progress (`rebase-apply/applying` exists).
    Am,
}

impl InProgressOperation {
    /// Human-readable description of the operation.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::RebaseInteractive => "interactive rebase",
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
            Self::Bisect => "bisect",
            Self::Am => "am",
        }
    }

    /// Hint text for how to continue or abort.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Merge => "fix conflicts and run \"git commit\"\n  (use \"git merge --abort\" to abort the merge)",
            Self::RebaseInteractive => "fix conflicts and then run \"git rebase --continue\"\n  (use \"git rebase --abort\" to abort the rebase)",
            Self::Rebase => "fix conflicts and then run \"git rebase --continue\"\n  (use \"git rebase --abort\" to abort the rebase)",
            Self::CherryPick => "fix conflicts and run \"git cherry-pick --continue\"\n  (use \"git cherry-pick --abort\" to abort the cherry-pick)",
            Self::Revert => "fix conflicts and run \"git revert --continue\"\n  (use \"git revert --abort\" to abort the revert)",
            Self::Bisect => "use \"git bisect reset\" to get back to the original branch",
            Self::Am => "fix conflicts and then run \"git am --continue\"\n  (use \"git am --abort\" to abort the am)",
        }
    }
}

/// Full snapshot of a repository's state.
///
/// This is the information that porcelain commands like `status` need to
/// display the repository's current situation.
#[derive(Debug, Clone)]
pub struct RepoState {
    /// Current HEAD state.
    pub head: HeadState,
    /// In-progress operations (there can be multiple, e.g. rebase + merge).
    pub in_progress: Vec<InProgressOperation>,
    /// Whether the repository is bare.
    pub is_bare: bool,
}

/// Resolve HEAD from the given git directory.
///
/// Reads `HEAD`, follows symbolic refs, and resolves the final OID.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory.
///
/// # Errors
///
/// Returns [`Error::Io`] if files cannot be read.
pub fn resolve_head(git_dir: &Path) -> Result<HeadState> {
    let head_path = git_dir.join("HEAD");
    let content = match fs::read_to_string(&head_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HeadState::Invalid),
        Err(e) => return Err(Error::Io(e)),
    };

    let trimmed = content.trim();

    if let Some(refname) = trimmed.strip_prefix("ref: ") {
        let refname = refname.to_owned();
        let short_name = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(&refname)
            .to_owned();

        // Try to resolve the ref to an OID
        let oid = resolve_ref(git_dir, &refname)?;

        Ok(HeadState::Branch {
            refname,
            short_name,
            oid,
        })
    } else {
        // Detached HEAD — should be a hex OID
        match ObjectId::from_hex(trimmed) {
            Ok(oid) => Ok(HeadState::Detached { oid }),
            Err(_) => Ok(HeadState::Invalid),
        }
    }
}

/// Resolve a ref name to an OID by reading the refs filesystem.
///
/// Follows symbolic refs and packed-refs.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory.
/// - `refname` — the full ref name (e.g. `refs/heads/main`).
///
/// # Returns
///
/// `Ok(Some(oid))` if the ref exists, `Ok(None)` if it doesn't (unborn),
/// or `Err` on I/O failure.
fn resolve_ref(git_dir: &Path, refname: &str) -> Result<Option<ObjectId>> {
    // Dispatch to reftable backend if configured
    if crate::reftable::is_reftable_repo(git_dir) {
        match crate::reftable::reftable_resolve_ref(git_dir, refname) {
            Ok(oid) => return Ok(Some(oid)),
            Err(_) => return Ok(None),
        }
    }

    let ref_path = git_dir.join(refname);

    // Try loose ref first
    match fs::read_to_string(&ref_path) {
        Ok(content) => {
            let trimmed = content.trim();
            // Follow symbolic ref chains
            if let Some(target) = trimmed.strip_prefix("ref: ") {
                return resolve_ref(git_dir, target);
            }
            match ObjectId::from_hex(trimmed) {
                Ok(oid) => Ok(Some(oid)),
                Err(_) => Ok(None),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Try packed-refs in git_dir
            if let Some(oid) = resolve_packed_ref(git_dir, refname)? {
                return Ok(Some(oid));
            }

            // For worktrees, fall back to the common git directory for
            // shared refs (branches, tags, etc.).
            if let Some(common) = common_dir_for(git_dir) {
                if common != git_dir {
                    // Try loose ref in common dir
                    let common_ref = common.join(refname);
                    match fs::read_to_string(&common_ref) {
                        Ok(content) => {
                            let trimmed = content.trim();
                            if let Some(target) = trimmed.strip_prefix("ref: ") {
                                return resolve_ref(git_dir, target);
                            }
                            match ObjectId::from_hex(trimmed) {
                                Ok(oid) => return Ok(Some(oid)),
                                Err(_) => {}
                            }
                        }
                        Err(e2) if e2.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e2)
                            if e2.kind() == std::io::ErrorKind::IsADirectory
                                || e2.kind() == std::io::ErrorKind::NotADirectory
                                || e2.raw_os_error() == Some(21)
                                || e2.raw_os_error() == Some(20) => {}
                        Err(e2) => return Err(Error::Io(e2)),
                    }
                    // Try packed-refs in common dir
                    return resolve_packed_ref(&common, refname);
                }
            }
            Ok(None)
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::IsADirectory
                || e.kind() == std::io::ErrorKind::NotADirectory
                || e.raw_os_error() == Some(21)
                || e.raw_os_error() == Some(20) =>
        {
            // Directory/file conflicts in refs (e.g. HEAD points at
            // refs/heads/outer while refs/heads/outer/inner exists) should be
            // treated like a missing ref, not a hard I/O failure.
            Ok(None)
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Determine the common git directory for worktree-aware ref resolution.
fn common_dir_for(git_dir: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let rel = raw.trim();
    let path = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        git_dir.join(rel)
    };
    path.canonicalize().ok()
}

/// Look up a ref in `packed-refs`.
fn resolve_packed_ref(git_dir: &Path, refname: &str) -> Result<Option<ObjectId>> {
    let packed_path = git_dir.join("packed-refs");
    let content = match fs::read_to_string(&packed_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        // Format: "<hex-oid> <refname>"
        if let Some((hex, name)) = line.split_once(' ') {
            if name == refname {
                if let Ok(oid) = ObjectId::from_hex(hex) {
                    return Ok(Some(oid));
                }
            }
        }
    }

    Ok(None)
}

/// Detect in-progress operations by checking for sentinel files.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory.
///
/// # Returns
///
/// A list of detected in-progress operations.
pub fn detect_in_progress(git_dir: &Path) -> Vec<InProgressOperation> {
    let mut ops = Vec::new();

    if git_dir.join("MERGE_HEAD").exists() {
        ops.push(InProgressOperation::Merge);
    }

    // Interactive rebase: rebase-merge/ directory
    let rebase_merge = git_dir.join("rebase-merge");
    if rebase_merge.is_dir() {
        if rebase_merge.join("interactive").exists() {
            ops.push(InProgressOperation::RebaseInteractive);
        } else {
            ops.push(InProgressOperation::Rebase);
        }
    }

    // Non-interactive rebase or am: rebase-apply/ directory
    let rebase_apply = git_dir.join("rebase-apply");
    if rebase_apply.is_dir() {
        if rebase_apply.join("applying").exists() {
            ops.push(InProgressOperation::Am);
        } else {
            ops.push(InProgressOperation::Rebase);
        }
    }

    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        ops.push(InProgressOperation::CherryPick);
    }

    if git_dir.join("REVERT_HEAD").exists() {
        ops.push(InProgressOperation::Revert);
    }

    if git_dir.join("BISECT_LOG").exists() {
        ops.push(InProgressOperation::Bisect);
    }

    ops
}

/// Build a complete [`RepoState`] snapshot for a repository.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory.
/// - `is_bare` — whether this is a bare repository.
///
/// # Errors
///
/// Returns [`Error::Io`] on filesystem failures.
pub fn repo_state(git_dir: &Path, is_bare: bool) -> Result<RepoState> {
    let head = resolve_head(git_dir)?;
    let in_progress = detect_in_progress(git_dir);

    Ok(RepoState {
        head,
        in_progress,
        is_bare,
    })
}

/// Read the MERGE_HEAD file and return the OIDs listed.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory.
///
/// # Returns
///
/// A vector of merge parent OIDs, or empty if not in a merge.
pub fn read_merge_heads(git_dir: &Path) -> Result<Vec<ObjectId>> {
    let path = git_dir.join("MERGE_HEAD");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io(e)),
    };

    let mut oids = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            oids.push(ObjectId::from_hex(trimmed)?);
        }
    }
    Ok(oids)
}

/// Read the MERGE_MSG file.
///
/// # Parameters
///
/// - `git_dir` — path to the `.git` directory.
///
/// # Returns
///
/// The merge message text, or `None` if not in a merge.
pub fn read_merge_msg(git_dir: &Path) -> Result<Option<String>> {
    let path = git_dir.join("MERGE_MSG");
    match fs::read_to_string(&path) {
        Ok(c) => Ok(Some(c)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(e)),
    }
}

/// Read CHERRY_PICK_HEAD.
pub fn read_cherry_pick_head(git_dir: &Path) -> Result<Option<ObjectId>> {
    read_single_oid_file(&git_dir.join("CHERRY_PICK_HEAD"))
}

/// Read REVERT_HEAD.
pub fn read_revert_head(git_dir: &Path) -> Result<Option<ObjectId>> {
    read_single_oid_file(&git_dir.join("REVERT_HEAD"))
}

/// Read ORIG_HEAD.
pub fn read_orig_head(git_dir: &Path) -> Result<Option<ObjectId>> {
    read_single_oid_file(&git_dir.join("ORIG_HEAD"))
}

/// Read a file that contains a single OID on its first line.
fn read_single_oid_file(path: &Path) -> Result<Option<ObjectId>> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(ObjectId::from_hex(trimmed)?))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(e)),
    }
}

/// Check upstream (tracking) information for the current branch.
///
/// Returns `(ahead, behind)` counts relative to the tracking branch.
/// This requires commit walking and is deferred for now.
///
/// # Parameters
///
/// - `_git_dir` — path to the `.git` directory.
/// - `_branch` — the local branch name.
///
/// # Returns
///
/// `None` if no upstream is configured.
pub fn upstream_tracking(_git_dir: &Path, _branch: &str) -> Result<Option<(usize, usize)>> {
    // TODO: Implement ahead/behind counting once config + rev-list integration is ready.
    Ok(None)
}
