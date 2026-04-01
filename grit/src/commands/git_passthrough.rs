//! Shared passthrough runner for maintenance subcommands.

use anyhow::{Context, Result};
use std::ffi::OsString;
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
    let status = Command::new(&git_bin)
        .arg(subcommand)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to execute {}", git_bin.to_string_lossy()))?;

    if status.success() {
        return Ok(());
    }

    std::process::exit(status.code().unwrap_or(1));
}
