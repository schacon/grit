//! `grit cherry-pick` — passthrough to the system Git binary.
//!
//! Cherry-pick applies the changes introduced by a commit onto the current
//! branch.  The full operation (conflict resolution, signoff, etc.) is
//! delegated to the real `git cherry-pick`.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit cherry-pick`.
#[derive(Debug, ClapArgs)]
#[command(about = "Apply the changes introduced by existing commits")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit cherry-pick` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("cherry-pick", &args.args)
}
