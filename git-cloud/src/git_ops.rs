//! Git helpers: discover repo root, normalize `origin` URL, merge remote branches.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// Returns the git work tree root using `git rev-parse --show-toplevel`, or `path` if provided.
pub fn resolve_repo_root(path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = path {
        return p
            .canonicalize()
            .with_context(|| format!("repo path {}", p.display()));
    }
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run `git rev-parse` — is this a git repository?")?;
    if !out.status.success() {
        anyhow::bail!(
            "`git rev-parse --show-toplevel` failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(PathBuf::from(s.trim()))
}

/// Runs a git subprocess in `repo` and returns trimmed stdout.
pub fn git_stdout(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !out.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Returns `origin` remote URL, normalized to an `https://github.com/...` style URL when possible.
pub fn origin_https_url(repo: &Path) -> Result<String> {
    let url = git_stdout(repo, &["remote", "get-url", "origin"])?;
    Ok(normalize_remote_url(&url))
}

fn normalize_remote_url(url: &str) -> String {
    let u = url.trim();
    if let Some(rest) = u.strip_prefix("git@github.com:") {
        let path = rest.trim_end_matches(".git");
        return format!("https://github.com/{path}");
    }
    u.trim_end_matches(".git").to_string()
}

/// `git fetch origin` (full fetch).
pub fn fetch_origin(repo: &Path) -> Result<()> {
    git_stdout(repo, &["fetch", "origin"]).map(|_| ())
}

/// True when `origin/<branch>` exists locally (after fetch).
pub fn origin_branch_exists(repo: &Path, branch: &str) -> bool {
    let spec = format!("origin/{branch}");
    Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "--verify", &spec])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Checks out `main` and pulls from `origin/main`.
pub fn checkout_and_pull_main(repo: &Path) -> Result<()> {
    git_stdout(repo, &["checkout", "main"])?;
    git_stdout(repo, &["pull", "origin", "main"])?;
    Ok(())
}

/// Merges `origin/<branch>` into the current branch.
/// Returns `Ok(true)` on success, `Ok(false)` if merge stopped with conflicts.
pub fn merge_origin_branch(repo: &Path, branch: &str) -> Result<bool> {
    let spec = format!("origin/{branch}");
    let out = Command::new("git")
        .current_dir(repo)
        .args([
            "merge",
            &spec,
            "--no-edit",
            "-m",
            &format!("merge: integrate cloud branch {branch}"),
        ])
        .output()
        .with_context(|| format!("git merge {spec}"))?;
    if out.status.success() {
        return Ok(true);
    }
    if has_unmerged_paths(repo)? {
        return Ok(false);
    }
    anyhow::bail!("git merge failed: {}", String::from_utf8_lossy(&out.stderr));
}

/// True when a merge is in progress (`MERGE_HEAD` exists).
pub fn merge_in_progress(repo: &Path) -> bool {
    Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Stages everything and concludes the merge with `--no-edit` if a merge is in progress.
pub fn conclude_merge_if_needed(repo: &Path) -> Result<()> {
    if !merge_in_progress(repo) {
        return Ok(());
    }
    git_stdout(repo, &["add", "-A"])?;
    let out = Command::new("git")
        .current_dir(repo)
        .args(["commit", "--no-edit"])
        .output()
        .context("git commit --no-edit")?;
    if !out.status.success() {
        anyhow::bail!(
            "git commit --no-edit failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Aborts the current merge/rebase if possible (best-effort).
pub fn abort_merge(repo: &Path) {
    let _ = Command::new("git")
        .current_dir(repo)
        .args(["merge", "--abort"])
        .status();
}

/// True if a merge or rebase is in progress (conflict state).
pub fn has_unmerged_paths(repo: &Path) -> Result<bool> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(["diff", "--name-only", "--diff-filter=U"])
        .output()
        .context("git diff for unmerged paths")?;
    if !out.status.success() {
        anyhow::bail!(
            "git diff --diff-filter=U failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(!out.stdout.is_empty())
}

/// Returns true when there is nothing to commit (clean index vs HEAD).
pub fn is_clean_worktree(repo: &Path) -> Result<bool> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(["status", "--porcelain"])
        .output()
        .context("git status --porcelain")?;
    if !out.status.success() {
        anyhow::bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(out.stdout.is_empty())
}
