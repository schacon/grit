//! Correctness checks for network benchmark clones (system `git` fsck + ref parity).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::bench_env::empty_global_config_path;

fn git_env(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty_global_config_path())
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
}

fn rev_parse(git: &Path, repo: &Path, spec: &str) -> Result<String> {
    let mut cmd = Command::new(git);
    cmd.args(["rev-parse", spec]).current_dir(repo);
    git_env(&mut cmd);
    let out = cmd
        .output()
        .with_context(|| format!("git rev-parse {spec} in {}", repo.display()))?;
    if !out.status.success() {
        bail!(
            "git rev-parse {spec} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn ref_oid(git: &Path, repo: &Path, refname: &str) -> Result<String> {
    let mut cmd = Command::new(git);
    cmd.args(["rev-parse", refname]).current_dir(repo);
    git_env(&mut cmd);
    let out = cmd
        .output()
        .with_context(|| format!("git rev-parse {refname} in {}", repo.display()))?;
    if !out.status.success() {
        bail!(
            "git rev-parse {refname} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `git fsck --strict --no-dangling` on `clone` and compare clone refs to `source_bare`.
///
/// Matches normal (non-mirror) clone semantics: every local branch and
/// `refs/remotes/origin/*` on the clone must match the corresponding ref on the bare remote,
/// and `HEAD` must match the bare default branch tip.
pub fn verify_clone_against_bare(
    git: &Path,
    source_bare: &Path,
    clone: &Path,
    default_branch: &str,
) -> Result<()> {
    let mut fsck = Command::new(git);
    fsck.args(["fsck", "--strict", "--no-dangling"])
        .current_dir(clone);
    git_env(&mut fsck);
    let out = fsck
        .output()
        .with_context(|| format!("git fsck in {}", clone.display()))?;
    if !out.status.success() {
        bail!(
            "git fsck failed on {}: {}",
            clone.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    let bare_tip = ref_oid(git, source_bare, &format!("refs/heads/{default_branch}"))?;
    let clone_head = rev_parse(git, clone, "HEAD")?;
    if bare_tip != clone_head {
        bail!("HEAD mismatch: bare {default_branch} is {bare_tip}, clone HEAD is {clone_head}");
    }

    let origin_ref = format!("refs/remotes/origin/{default_branch}");
    let origin_oid = ref_oid(git, clone, &origin_ref)
        .with_context(|| format!("clone missing remote-tracking ref {origin_ref} after clone"))?;
    if origin_oid != bare_tip {
        bail!("origin/{default_branch} mismatch: bare {bare_tip} != clone {origin_oid}");
    }

    let mut cmd = Command::new(git);
    cmd.args([
        "for-each-ref",
        "--format=%(refname)",
        "refs/heads",
        "refs/remotes/origin",
    ])
    .current_dir(clone);
    git_env(&mut cmd);
    let out = cmd.output().context("git for-each-ref on clone")?;
    if !out.status.success() {
        bail!(
            "git for-each-ref failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let name = line.trim();
        if name.is_empty() {
            continue;
        }
        let clone_oid = ref_oid(git, clone, name)?;
        let bare_ref = if let Some(branch) = name.strip_prefix("refs/remotes/origin/") {
            if branch == "HEAD" {
                continue;
            }
            format!("refs/heads/{branch}")
        } else if name.starts_with("refs/heads/") {
            name.to_string()
        } else {
            continue;
        };
        let bare_oid = ref_oid(git, source_bare, &bare_ref)?;
        if clone_oid != bare_oid {
            bail!("ref mismatch for {name}: clone {clone_oid} != bare {bare_ref} {bare_oid}");
        }
    }
    Ok(())
}

fn rev_list_count(git: &Path, repo: &Path, spec: &str) -> Result<u64> {
    let mut cmd = Command::new(git);
    cmd.args(["rev-list", "--count", spec]).current_dir(repo);
    git_env(&mut cmd);
    let out = cmd
        .output()
        .with_context(|| format!("git rev-list --count {spec} in {}", repo.display()))?;
    if !out.status.success() {
        bail!(
            "git rev-list --count failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .with_context(|| format!("parse rev-list count {:?}", out.stdout))
}

fn resolve_git_dir(worktree: &Path) -> Result<PathBuf> {
    let dot_git = worktree.join(".git");
    if dot_git.is_dir() {
        return Ok(dot_git);
    }
    if dot_git.is_file() {
        let content = fs::read_to_string(&dot_git).context("read .git file")?;
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("gitdir:") {
                let path = PathBuf::from(rest.trim());
                return Ok(if path.is_absolute() {
                    path
                } else {
                    worktree.join(path)
                });
            }
        }
    }
    bail!("could not resolve git directory for {}", worktree.display());
}

/// Depth-1 / `--shallow` clone semantics: refs match the bare remote tip, history is truncated,
/// and `.git/shallow` lists the boundary commit(s).
pub fn verify_shallow_depth_one_clone(
    git: &Path,
    source_bare: &Path,
    clone: &Path,
    default_branch: &str,
) -> Result<()> {
    verify_clone_against_bare(git, source_bare, clone, default_branch)?;

    let head = rev_parse(git, clone, "HEAD")?;
    let reachable = rev_list_count(git, clone, "HEAD")?;
    if reachable != 1 {
        bail!(
            "shallow clone rev-list --count HEAD expected 1, got {reachable} (full clone has deeper history)"
        );
    }

    let bare_ref = format!("refs/heads/{default_branch}");
    let bare_depth = rev_list_count(git, source_bare, &bare_ref)?;
    if bare_depth <= 1 {
        bail!("bare remote must have >1 commits for shallow-depth1 scenario (got {bare_depth})");
    }

    let shallow_path = resolve_git_dir(clone)?.join("shallow");
    if !shallow_path.is_file() {
        bail!(
            "expected shallow boundary file {} (absent on full clones)",
            shallow_path.display()
        );
    }
    let boundaries: HashSet<String> = fs::read_to_string(&shallow_path)
        .with_context(|| format!("read {}", shallow_path.display()))?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if boundaries.is_empty() {
        bail!(".git/shallow is empty");
    }
    if !boundaries.contains(&head) {
        bail!(
            "HEAD {head} not listed in .git/shallow ({} boundaries)",
            boundaries.len()
        );
    }
    Ok(())
}

/// After fetch, the client's remote-tracking branch must match the bare remote tip.
pub fn verify_client_fetch_tip(
    git: &Path,
    source_bare: &Path,
    client: &Path,
    default_branch: &str,
) -> Result<()> {
    let bare_oid = ref_oid(git, source_bare, &format!("refs/heads/{default_branch}"))?;
    let client_oid = ref_oid(
        git,
        client,
        &format!("refs/remotes/origin/{default_branch}"),
    )?;
    if bare_oid != client_oid {
        bail!("fetch tip mismatch: bare {bare_oid} != origin/{default_branch} {client_oid}");
    }
    Ok(())
}
