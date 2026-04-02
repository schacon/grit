//! `grit hook` — run git hooks.
//!
//! Runs a hook script from `.git/hooks/` or the directory configured
//! via `core.hooksPath`.
//!
//! Usage:
//!   grit hook run <hook-name> [-- <hook-args>...]

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

/// Arguments for `grit hook`.
#[derive(Debug, ClapArgs)]
#[command(about = "Run git hooks")]
pub struct Args {
    #[command(subcommand)]
    pub action: HookAction,
}

#[derive(Debug, Subcommand)]
pub enum HookAction {
    /// Run a hook.
    Run(RunArgs),
}

/// Arguments for `grit hook run`.
#[derive(Debug, ClapArgs)]
pub struct RunArgs {
    /// Name of the hook to run (e.g. pre-commit, post-merge).
    pub hook_name: String,

    /// Arguments to pass to the hook script.
    #[arg(last = true)]
    pub hook_args: Vec<String>,
}

/// Run the `hook` command.
pub fn run(args: Args) -> Result<()> {
    match args.action {
        HookAction::Run(run_args) => run_hook(run_args),
    }
}

fn run_hook(args: RunArgs) -> Result<()> {
    let repo = Repository::discover(None)?;

    // Determine hooks directory: core.hooksPath or default .git/hooks.
    let hooks_dir = resolve_hooks_dir(&repo)?;
    let hook_path = hooks_dir.join(&args.hook_name);

    // If the hook doesn't exist, silently succeed (git behaviour).
    if !hook_path.exists() {
        return Ok(());
    }

    // Check if executable.
    let meta = fs::metadata(&hook_path)
        .with_context(|| format!("stat {}", hook_path.display()))?;
    if meta.permissions().mode() & 0o111 == 0 {
        // Not executable — silently skip, matching git behavior.
        return Ok(());
    }

    let status = Command::new(&hook_path)
        .args(&args.hook_args)
        .current_dir(
            repo.work_tree
                .as_deref()
                .unwrap_or(&repo.git_dir),
        )
        .env("GIT_DIR", &repo.git_dir)
        .status()
        .with_context(|| format!("running hook {}", hook_path.display()))?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        bail!(
            "hook '{}' exited with status {}",
            args.hook_name,
            code
        );
    }

    Ok(())
}

/// Resolve the hooks directory from config or fall back to `$GIT_DIR/hooks`.
fn resolve_hooks_dir(repo: &Repository) -> Result<PathBuf> {
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    if let Some(hooks_path) = config.get("core.hooksPath") {
        let expanded = grit_lib::config::parse_path(&hooks_path);
        let p = PathBuf::from(expanded);
        if p.is_absolute() {
            return Ok(p);
        }
        // Relative to the working directory (git behaviour).
        let cwd = std::env::current_dir()?;
        return Ok(cwd.join(p));
    }

    Ok(repo.git_dir.join("hooks"))
}
