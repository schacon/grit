//! `grit http-push` — push objects to a remote repository via HTTP/DAV.
//!
//! Pushes objects to a remote repository using HTTP/WebDAV.
//! Currently a stub that reports the feature is not implemented.
//!
//!     grit http-push <URL>

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit http-push`.
#[derive(Debug, ClapArgs)]
#[command(about = "Push objects over HTTP/DAV to another repository")]
pub struct Args {
    /// URL of the remote repository.
    #[arg(value_name = "URL")]
    pub url: String,

    /// Refs to push.
    #[arg(value_name = "REF")]
    pub refs: Vec<String>,

    /// Report what would be pushed without actually doing it.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Verbose output.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Run `grit http-push`.
pub fn run(args: Args) -> Result<()> {
    bail!(
        "http-push to '{}' is not yet implemented in grit",
        args.url
    )
}
