//! Shared runner for delegating to the system Git binary.

use anyhow::{bail, Context, Result};
use grit_lib::repo::Repository;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Execute a Git subcommand with arguments using the system Git binary.
///
/// The binary path is taken from `REAL_GIT` when set, otherwise `/usr/bin/git`.
/// Standard I/O is inherited so command output and errors remain Git-compatible.
///
/// # Parameters
///
/// - `subcommand` - Subcommand name such as `repack` or `gc`.
/// - `args` - Raw CLI arguments to pass through unchanged.
///
/// # Errors
///
/// Returns an error when the subprocess cannot be spawned or waited on.
pub fn run(subcommand: &str, args: &[String]) -> Result<()> {
    let mut full_args = Vec::with_capacity(args.len() + 1);
    full_args.push(subcommand.to_owned());
    full_args.extend(args.iter().cloned());
    run_args_in_current_cwd(&full_args)
}

fn run_args_in_current_cwd(args: &[String]) -> Result<()> {
    let cwd = std::env::current_dir().ok();
    run_args_with_cwd(args, cwd.as_deref())
}

fn run_args_in_original_cwd(args: &[String]) -> Result<()> {
    let orig = std::env::var_os("GRIT_ORIG_CWD")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    run_args_with_cwd(args, orig.as_deref())
}

fn run_args_with_cwd(args: &[String], cwd: Option<&Path>) -> Result<()> {
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let mut cmd = Command::new(&git_bin);
    cmd.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
        cmd.env("PWD", dir);
    }

    // Ensure nested `git` subprocesses invoked by system Git also resolve to
    // the real Git binary (not the test harness shim that points at `grit`).
    if let Some(parent) = Path::new(&git_bin).parent() {
        let mut paths = vec![parent.to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to execute {}", git_bin.to_string_lossy()))?;

    if status.success() {
        return Ok(());
    }

    std::process::exit(status.code().unwrap_or(1));
}

/// Execute a subcommand using system Git with the invocation's original argv
/// tail (everything after `subcommand`).
pub fn run_current_invocation(subcommand: &str) -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let Some(_idx) = argv.iter().position(|arg| arg == subcommand) else {
        bail!("failed to determine {subcommand} arguments");
    };
    let passthrough_args = argv.get(1..).map(|s| s.to_vec()).unwrap_or_default();
    run_args_in_original_cwd(&passthrough_args)
}

/// Return true when command execution started from a subdirectory of the
/// worktree (using `$PWD` when available for shell-parity), which is where
/// native Git has subtle safeguards around removing the current directory.
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

/// Returns true when a pathspec references a parent directory (`..`), which
/// requires nuanced outside-prefix semantics that are best delegated to native Git.
pub fn has_parent_pathspec_component(pathspec: &str) -> bool {
    Path::new(pathspec).components().any(|component| {
        matches!(component, std::path::Component::ParentDir)
    })
}
