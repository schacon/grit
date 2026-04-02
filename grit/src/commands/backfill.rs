//! `grit backfill` — download missing blobs for partial clone.
//!
//! Walks the reachable trees and identifies any missing blob objects
//! that need to be fetched from the promisor remote.  Currently a
//! basic stub that delegates to the system Git binary.
//!
//!     grit backfill

use crate::commands::git_passthrough;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit backfill`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download missing blobs for partial clone")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit backfill` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    git_passthrough::run("backfill", &args.args)
}
