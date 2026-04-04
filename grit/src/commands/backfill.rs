//! `grit backfill` — download missing blobs for partial clone.
//!
//! Walks the reachable trees and identifies any missing blob objects
//! that need to be fetched from the promisor remote.
//!
//! Since grit does not create true partial clones (--filter is accepted
//! but all objects are fetched), there are never missing blobs and
//! backfill is a successful no-op.
//!
//!     grit backfill

use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit backfill`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download missing blobs for partial clone")]
pub struct Args {
    /// Minimum batch size.
    #[arg(long = "min-batch-size", value_name = "N")]
    pub min_batch_size: Option<usize>,

    /// Remaining raw arguments.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit backfill`.
///
/// Since grit always fetches all objects during clone (even with --filter),
/// there are no missing blobs.  Backfill is a successful no-op.
pub fn run(_args: Args) -> Result<()> {
    // Nothing to do — all objects are already present.
    Ok(())
}
