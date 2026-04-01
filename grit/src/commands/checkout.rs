//! `grit checkout` — passthrough to the system Git binary.
//!
//! Branch switching, file restoration, and detached-HEAD operations are
//! forwarded to the real `git checkout` so that the complete set of
//! working-tree and ref-update semantics is available.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Switch branches or restore working tree files")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit checkout` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("checkout", &args.args)
}
