//! `grit diff-pairs` — compare pairs of blobs or trees.
//!
//! A newer plumbing command that reads pairs of object IDs from stdin
//! and outputs the diff for each pair.
//!
//!     echo "<oid1> <oid2>" | grit diff-pairs

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit diff-pairs`.
#[derive(Debug, ClapArgs)]
#[command(about = "Compare pairs of blobs/trees read from stdin")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit diff-pairs` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("diff-pairs", &args.args)
}
