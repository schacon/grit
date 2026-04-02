//! `grit replay` — replay commits on a new base.
//!
//! Replays a range of commits onto a new base, effectively cherry-picking
//! each commit in sequence.  Usage:
//!
//!     grit replay --onto <newbase> <old>..<new>
//!
//! Delegates to the system Git binary which uses the cherry-pick
//! infrastructure internally.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit replay`.
#[derive(Debug, ClapArgs)]
#[command(about = "Replay commits on a new base")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit replay` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("replay", &args.args)
}
