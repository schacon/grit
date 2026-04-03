//! `grit hook` — run git hooks.
//!
//! Runs a hook script from `.git/hooks/` or the directory configured
//! via `core.hooksPath`.
//!
//! Usage:
//!   grit hook run <hook-name> [-- <hook-args>...]

use anyhow::{bail, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::repo::Repository;

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
        HookAction::Run(run_args) => run_hook_cmd(run_args),
    }
}

fn run_hook_cmd(args: RunArgs) -> Result<()> {
    let repo = Repository::discover(None)?;
    let hook_args: Vec<&str> = args.hook_args.iter().map(|s| s.as_str()).collect();

    match run_hook(&repo, &args.hook_name, &hook_args, None) {
        HookResult::Success | HookResult::NotFound => Ok(()),
        HookResult::Failed(code) => {
            bail!("hook '{}' exited with status {}", args.hook_name, code);
        }
    }
}
