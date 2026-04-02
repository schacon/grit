//! `grit repo` — manage repository metadata.
//!
//! A newer Git command for managing repository-level metadata and
//! configuration.  Currently a basic stub.
//!
//!     grit repo [<subcommand>]

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit repo`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage repository metadata")]
pub struct Args {
    /// Subcommand (e.g. info, health).
    #[arg(value_name = "SUBCOMMAND")]
    pub subcommand: Option<String>,

    /// Additional arguments.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit repo`.
pub fn run(args: Args) -> Result<()> {
    match args.subcommand.as_deref() {
        Some(sub) => bail!("repo subcommand '{}' is not yet implemented in grit", sub),
        None => bail!("repo: no subcommand specified"),
    }
}
