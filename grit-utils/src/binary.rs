//! Resolve executables on `PATH` without shelling out to `which`.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Resolve a binary: explicit user path, workspace `target/release/grit`, then `PATH`.
///
/// Explicit paths are made absolute and canonicalized so they remain valid after the
/// harness changes working directory for hyperfine runs.
pub fn resolve_binary(name: &str, user_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = user_path {
        return canonicalize_executable(p);
    }
    if name == "grit" {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/release/grit");
        if workspace.is_file() {
            return workspace.canonicalize().context("canonicalize grit path");
        }
    }
    if name == "grit-http-server" {
        let workspace =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/release/grit-http-server");
        if workspace.is_file() {
            return workspace
                .canonicalize()
                .context("canonicalize grit-http-server path");
        }
    }
    find_on_path(name).with_context(|| format!("could not find `{name}` on PATH"))
}

fn canonicalize_executable(p: &Path) -> Result<PathBuf> {
    let abs = if p.is_relative() {
        env::current_dir()
            .context("resolve relative executable path")?
            .join(p)
    } else {
        p.to_path_buf()
    };
    if !abs.is_file() {
        bail!("binary not found: {}", p.display());
    }
    abs.canonicalize()
        .with_context(|| format!("canonicalize executable {}", p.display()))
}

/// Locate `hyperfine` or fail with an actionable message.
/// Resolve `grit-http-server` (explicit path, workspace release build, then `PATH`).
pub fn resolve_http_server(user_path: Option<&Path>) -> Result<PathBuf> {
    resolve_binary("grit-http-server", user_path)
}

pub fn require_hyperfine() -> Result<PathBuf> {
    find_on_path("hyperfine").context(
        "hyperfine is required for grit-bench but was not found on PATH. \
         Install it from https://github.com/sharkdp/hyperfine",
    )
}

/// Run `program --version` (or `-V` for grit) and return stdout trimmed.
pub fn tool_version(program: &Path) -> String {
    let output = std::process::Command::new(program)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .or_else(|| {
            std::process::Command::new(program)
                .arg("-V")
                .output()
                .ok()
                .filter(|o| o.status.success())
        });
    output
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Resolve `spec` to a commit object id, or `None` when it is not a committish.
fn rev_parse_commit(repo: &Path, spec: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &format!("{spec}^{{commit}}")])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn git_current_branch(repo: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn newest_branch_tip_commit(repo: &Path, ref_prefix: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args([
            "for-each-ref",
            ref_prefix,
            "--format=%(objectname)",
            "--sort=-committerdate",
        ])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let hash = line.trim();
        if hash.is_empty() {
            continue;
        }
        if rev_parse_commit(repo, hash).is_some() {
            return Some(hash.to_owned());
        }
    }
    None
}

/// Best-effort commit hash for the grit tree (when this crate is built from the grit repo).
///
/// Prefers the checked-out branch tip when it is a real commit (not `gitbutler/workspace`),
/// then the newest `refs/heads/factory/*` tip, then other branch tips, then `HEAD`.
pub fn grit_source_commit() -> Option<String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    if let Some(branch) = git_current_branch(&repo) {
        if branch != "gitbutler/workspace" {
            if let Some(h) = rev_parse_commit(&repo, &format!("refs/heads/{branch}")) {
                return Some(h);
            }
        }
    }
    if let Some(h) = newest_branch_tip_commit(&repo, "refs/heads/factory/") {
        return Some(h);
    }
    if let Some(h) = newest_branch_tip_commit(&repo, "refs/heads/") {
        return Some(h);
    }
    rev_parse_commit(&repo, "HEAD")
}

fn find_on_path(name: &str) -> Result<PathBuf> {
    let paths = env::var_os("PATH").context("PATH is not set")?;
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .with_context(|| format!("canonicalize `{}` on PATH", candidate.display()));
        }
    }
    bail!("`{name}` not found on PATH")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn grit_source_commit_is_committish() {
        let Some(commit) = grit_source_commit() else {
            return;
        };
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let ok = Command::new("git")
            .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
            .current_dir(&repo)
            .status()
            .expect("git cat-file");
        assert!(ok.success(), "grit_source_commit must resolve to a commit object");
    }

    #[test]
    fn relative_explicit_path_is_canonicalized() {
        let original_cwd = env::current_dir().expect("initial cwd");
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("fake-grit");
        fs::write(&script, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let rel = Path::new("fake-grit");
        env::set_current_dir(dir.path()).expect("set cwd for relative path test");
        let resolved = resolve_binary("grit", Some(rel)).unwrap();
        env::set_current_dir(&original_cwd).expect("restore cwd after relative path test");
        assert!(resolved.is_absolute());
        assert_eq!(resolved, script.canonicalize().unwrap());
    }
}
