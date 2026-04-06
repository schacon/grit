//! Shared passthrough runner for maintenance subcommands.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::Path;
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
    let git_bin = std::env::var_os("REAL_GIT").unwrap_or_else(|| OsString::from("/usr/bin/git"));
    let mut cmd = Command::new(&git_bin);
    cmd.arg(subcommand)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

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
