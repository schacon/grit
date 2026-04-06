//! `grit hook` — run git hooks.
//!
//! Runs a hook script from `.git/hooks/` or the directory configured
//! via `core.hooksPath`.
//!
//! Usage:
//!   grit hook run <hook-name> [-- <hook-args>...]

use anyhow::Result;
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
    /// Exit successfully when the requested hook does not exist.
    #[arg(long = "ignore-missing")]
    pub ignore_missing: bool,

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
        HookResult::Success => Ok(()),
        HookResult::NotFound if args.ignore_missing => Ok(()),
        HookResult::NotFound => {
            eprintln!("error: cannot find a hook named {}", args.hook_name);
            std::process::exit(1);
        }
        HookResult::Failed(code) => std::process::exit(code),
    }
}
