//! `grit switch` — passthrough to the system Git binary.
//!
//! Branch creation, switching, and orphan-branch operations are forwarded to
//! the real `git switch` so that the complete set of semantics is available.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit switch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Switch branches")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit switch` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("switch", &args.args)
}
