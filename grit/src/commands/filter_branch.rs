//! `grit filter-branch` — rewrite branches by delegating to the system's
//! `git-filter-branch` shell script.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::process::Command;

/// Arguments for `grit filter-branch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Rewrite branches (delegates to system git-filter-branch)")]
pub struct Args {
    /// Raw arguments forwarded to git-filter-branch.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub fn run(args: Args) -> Result<()> {
    // Find the system git-filter-branch script.
    // Try GIT_EXEC_PATH first, then well-known locations.
    let exec_path = std::env::var("GIT_EXEC_PATH").ok().unwrap_or_else(|| {
        // Check common locations for the git exec path
        for candidate in &[
            "/usr/lib/git-core",
            "/usr/libexec/git-core",
            "/usr/local/lib/git-core",
            "/usr/local/libexec/git-core",
        ] {
            let p = std::path::Path::new(candidate).join("git-filter-branch");
            if p.exists() {
                return candidate.to_string();
            }
        }
        "/usr/lib/git-core".to_string()
    });

    let script_path = std::path::Path::new(&exec_path).join("git-filter-branch");
    if !script_path.exists() {
        anyhow::bail!(
            "cannot find git-filter-branch at {}",
            script_path.display()
        );
    }

    // Prepend the exec path to PATH so that `git-sh-setup` and other
    // shell helpers sourced by filter-branch can be found.
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{exec_path}:{current_path}");

    let status = Command::new("bash")
        .arg(script_path)
        .args(&args.args)
        .env("PATH", &new_path)
        .status()
        .context("failed to run git-filter-branch")?;

    std::process::exit(status.code().unwrap_or(1));
}
