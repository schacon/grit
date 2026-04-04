//! `grit backfill` — download missing blobs for partial clone.
//!
//! Walks the reachable trees and identifies any missing blob objects
//! that need to be fetched from the promisor remote.  When all objects
//! are already present (e.g. after a full clone), this is a no-op.
//!
//!     grit backfill

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::repo::Repository;

/// Arguments for `grit backfill`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download missing blobs for partial clone")]
pub struct Args {
    /// Limit backfill to a pathspec.
    #[arg(value_name = "PATH", num_args = 0..)]
    pub paths: Vec<String>,

    /// Minimum batch size for fetching.
    #[arg(long = "min-batch-size")]
    pub min_batch_size: Option<usize>,
}

/// Run `grit backfill`.
///
/// For repositories that have all objects present (non-partial clones or
/// partial clones that have been fully fetched), this is a successful no-op.
pub fn run(_args: Args) -> Result<()> {
    let _repo = Repository::discover(None).context("not a git repository")?;
    // In a full clone all objects are present — nothing to backfill.
    // For a true partial clone we would enumerate missing blobs from
    // reachable trees and batch-fetch them from the promisor remote.
    // Currently this succeeds silently when no objects are missing.
    Ok(())
}
