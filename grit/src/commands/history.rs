//! `grit history` — show commit history.
//!
//! An alias-like command that delegates to `log` with different defaults.
//! Equivalent to `grit log` but intended as a more user-friendly entry
//! point for browsing commit history.
//!
//!     grit history [<options>] [<revision>...]

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit history`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show commit history")]
pub struct Args {
    /// Raw arguments forwarded to `git log`.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit history` by delegating to `git log`.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("log", &args.args)
}
