//! `grit fast-import` — import from a fast-export stream.
//!
//! Delegates to [`grit_lib::fast_import::import_stream_with_options`] for the
//! supported command subset (blobs, commits, reset, done, merge parents).

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::fast_import::{self, FastImportOptions};
use grit_lib::repo::Repository;
use std::io;

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments (reserved for future import options).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-import`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let force = args.args.iter().any(|a| a == "--force");
    let stdin = io::stdin();
    let reader = stdin.lock();
    fast_import::import_stream_with_options(&repo, reader, FastImportOptions { force })
        .map_err(|e| anyhow::anyhow!("{e}"))
}
