//! `grit prune-packed` command.
//!
//! Removes loose objects from the object database that are already stored in
//! a pack file.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::prune_packed::{prune_packed_objects, PrunePackedOptions};
use grit_lib::repo::Repository;

/// Arguments for `grit prune-packed`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Do not remove anything; just show what would be removed.
    #[arg(short = 'n', long = "dry-run")]
    pub dry_run: bool,

    /// Suppress progress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,
}

/// Run `grit prune-packed`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("failed to discover repository")?;
    let objects_dir = repo.git_dir.join("objects");
    let opts = PrunePackedOptions {
        dry_run: args.dry_run,
        quiet: args.quiet,
    };
    prune_packed_objects(&objects_dir, opts).context("prune-packed failed")?;
    Ok(())
}
