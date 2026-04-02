//! `grit http-fetch` — download from a remote Git repository via HTTP.
//!
//! Fetches objects from a remote repository using the dumb HTTP protocol.
//! Currently a stub that reports the feature is not implemented.
//!
//!     grit http-fetch <URL>

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit http-fetch`.
#[derive(Debug, ClapArgs)]
#[command(about = "Download from a remote Git repository via HTTP")]
pub struct Args {
    /// URL of the remote repository.
    #[arg(value_name = "URL")]
    pub url: String,

    /// Fetch a specific commit ID.
    #[arg(value_name = "COMMIT-ID")]
    pub commit_id: Option<String>,

    /// Verbosely report all fetched objects.
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Write the commit-id into the specified filename under $GIT_DIR.
    #[arg(short = 'a', value_name = "FILE")]
    pub append: Option<String>,
}

/// Run `grit http-fetch`.
pub fn run(args: Args) -> Result<()> {
    bail!(
        "http-fetch from '{}' is not yet implemented in grit",
        args.url
    )
}
