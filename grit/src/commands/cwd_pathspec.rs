//! Pure helpers for deciding when worktree-relative semantics depend on cwd.
//!
//! These do not spawn subprocesses. Commands use them to match Git’s edge-case
//! behavior without delegating to an external `git` binary.

use grit_lib::repo::Repository;
use std::path::{Path, PathBuf};

/// Returns true when command execution started from a subdirectory of the
/// worktree (using `$PWD` when available for shell-parity).
#[must_use]
pub fn should_passthrough_from_subdir(repo: &Repository) -> bool {
    let Some(work_tree) = repo.work_tree.as_ref() else {
        return false;
    };
    let cwd = std::env::current_dir().ok();
    let orig_cwd = std::env::var_os("GRIT_ORIG_CWD")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.canonicalize().ok());
    let pwd = std::env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .and_then(|p| p.canonicalize().ok());
    let wt_canon = work_tree
        .canonicalize()
        .unwrap_or_else(|_| work_tree.clone());
    let cwd_canon = cwd.and_then(|c| c.canonicalize().ok());
    let effective_cwd = orig_cwd
        .or(pwd)
        .or(cwd_canon)
        .unwrap_or_else(|| wt_canon.clone());
    effective_cwd.starts_with(&wt_canon) && effective_cwd != wt_canon
}

/// Returns true when a pathspec references a parent directory (`..`).
#[must_use]
pub fn has_parent_pathspec_component(pathspec: &str) -> bool {
    Path::new(pathspec)
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}
