//! `grit multi-pack-index` — manage multi-pack index files.
//!
//! Subcommands: `write` (create MIDX), `verify` (check MIDX integrity),
//! `repack` (repack using MIDX).  Enumerates packs and writes a combined
//! index for efficient multi-pack lookups.

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit multi-pack-index`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage multi-pack index")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit multi-pack-index` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("multi-pack-index", &args.args)
}
