//! Shared helpers for discovering branch refs occupied by worktrees.

use grit_lib::repo::Repository;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Return the canonical common git directory for this repository/worktree.
pub fn common_git_dir(repo: &Repository) -> PathBuf {
    if repo.git_dir.join("commondir").exists() {
        let common = fs::read_to_string(repo.git_dir.join("commondir")).unwrap_or_default();
        let common = common.trim();
        if common.is_empty() {
            return repo.git_dir.clone();
        }
        if Path::new(common).is_absolute() {
            PathBuf::from(common)
        } else {
            repo.git_dir
                .join(common)
                .canonicalize()
                .unwrap_or_else(|_| repo.git_dir.join(common))
        }
    } else {
        repo.git_dir.clone()
    }
}

/// Resolve the worktree path string from a worktree admin directory.
pub fn worktree_path_from_admin(admin_dir: &Path) -> String {
    if let Ok(gitdir_content) = fs::read_to_string(admin_dir.join("gitdir")) {
        let p = gitdir_content.trim().to_string();
        return Path::new(&p)
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or(p);
    }
    admin_dir.display().to_string()
}

/// Build a map `refs/heads/<name>` -> worktree-path for all refs currently
/// occupied by the main worktree and linked worktrees.
///
/// Includes:
/// - branch checked out via `HEAD` symref
/// - branch in bisect state (`BISECT_START`)
/// - rebase refs from `rebase-apply/head-name` and `rebase-merge/head-name`
/// - `rebase-merge/onto` when it is itself a `refs/heads/*` ref name
pub fn occupied_branch_refs(repo: &Repository) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let common = common_git_dir(repo);
    let main_wt_path = common
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| common.display().to_string());

    collect_from_admin(&common, &main_wt_path, &mut out);

    let worktrees_dir = common.join("worktrees");
    if let Ok(entries) = fs::read_dir(&worktrees_dir) {
        for entry in entries.flatten() {
            let admin = entry.path();
            let wt_path = worktree_path_from_admin(&admin);
            collect_from_admin(&admin, &wt_path, &mut out);
        }
    }

    out
}

fn collect_from_admin(admin_dir: &Path, wt_path: &str, out: &mut HashMap<String, String>) {
    // HEAD symref
    if let Ok(head_content) = fs::read_to_string(admin_dir.join("HEAD")) {
        let head_trimmed = head_content.trim();
        if let Some(refname) = head_trimmed.strip_prefix("ref: ") {
            if refname.starts_with("refs/heads/") {
                out.insert(refname.to_string(), wt_path.to_string());
            }
        }
    }

    // BISECT_START
    if admin_dir.join("BISECT_LOG").exists() {
        if let Ok(start) = fs::read_to_string(admin_dir.join("BISECT_START")) {
            let trimmed = start.trim();
            if !trimmed.is_empty() {
                let refname = if trimmed.starts_with("refs/heads/") {
                    trimmed.to_string()
                } else if trimmed.starts_with("refs/") {
                    String::new()
                } else {
                    format!("refs/heads/{trimmed}")
                };
                if !refname.is_empty() {
                    out.insert(refname, wt_path.to_string());
                }
            }
        }
    }

    // rebase-apply/head-name
    let rebase_apply = admin_dir.join("rebase-apply");
    if rebase_apply.exists() {
        if let Ok(head_name) = fs::read_to_string(rebase_apply.join("head-name")) {
            let refname = head_name.trim();
            if refname.starts_with("refs/heads/") {
                out.insert(refname.to_string(), wt_path.to_string());
            }
        }
    }

    // rebase-merge/head-name and onto
    let rebase_merge = admin_dir.join("rebase-merge");
    if rebase_merge.exists() {
        if let Ok(head_name) = fs::read_to_string(rebase_merge.join("head-name")) {
            let refname = head_name.trim();
            if refname.starts_with("refs/heads/") {
                out.insert(refname.to_string(), wt_path.to_string());
            }
        }
        if let Ok(onto) = fs::read_to_string(rebase_merge.join("onto")) {
            let onto = onto.trim();
            if onto.starts_with("refs/heads/") {
                out.insert(onto.to_string(), wt_path.to_string());
            }
        }
        if let Ok(update_refs) = fs::read_to_string(rebase_merge.join("update-refs")) {
            for line in update_refs.lines() {
                let refname = line.split_whitespace().next().unwrap_or("");
                if refname.starts_with("refs/heads/") {
                    out.insert(refname.to_string(), wt_path.to_string());
                }
            }
        }
    }
}

/// Whether an error string indicates branch/ref protection due to worktree use.
#[allow(dead_code)]
pub fn is_worktree_ref_protection_error(err: &str) -> bool {
    err.contains("cannot force update the branch")
        || err.contains("cannot delete branch")
        || err.contains("used by worktree")
        || err.contains("refusing to fetch into branch")
}

/// Decide whether `rebase` should be retried via system Git passthrough.
#[allow(dead_code)]
pub fn needs_passthrough_for_rebase(err: &str, argv_rest: &[String]) -> bool {
    if is_worktree_ref_protection_error(err) {
        return true;
    }
    if argv_rest
        .iter()
        .any(|a| a == "--update-refs" || a == "--interactive" || a == "-i")
    {
        return true;
    }
    err.contains("interactive rebase not fully supported")
        || err.contains("unexpected argument '--update-refs'")
}
