//! `grit unpack-objects` — unpack a pack stream into loose objects.
//!
//! Reads a PACK-format byte stream from stdin, validates its checksum, and
//! writes every object as a loose file in the repository's object database.
//! Delta objects are resolved automatically.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::io;

use grit_lib::repo::Repository;
use grit_lib::unpack_objects::{unpack_objects, UnpackOptions};

/// Arguments for `grit unpack-objects`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Dry run: parse and validate objects but do not write them.
    #[arg(short = 'n')]
    pub dry_run: bool,

    /// Quiet: suppress informational output.
    #[arg(short = 'q')]
    pub quiet: bool,

    /// Enable strict checking (accepted for compatibility; basic validation
    /// is always performed).
    #[arg(long)]
    pub strict: bool,
}

/// Run `grit unpack-objects`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let opts = UnpackOptions {
        dry_run: args.dry_run,
        quiet: args.quiet,
        strict: args.strict,
    };

    let mut stdin = io::stdin().lock();
    let count = unpack_objects(&mut stdin, &repo.odb, &opts).context("unpack-objects failed")?;

    if !args.quiet {
        eprintln!("Unpacking objects: done ({count} objects)");
    }

    Ok(())
}
