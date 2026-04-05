//! `grit history` — placeholder subcommand-based history browser.
//!
//! Currently has no subcommands and exits with an error asking for one.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit history`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show commit history")]
pub struct Args {
    /// Subcommand or arguments.
    #[arg(value_name = "ARG", num_args = 0..)]
    pub args: Vec<String>,
}

/// Run `grit history`.
pub fn run(args: Args) -> Result<()> {
    if args.args.is_empty() {
        bail!("need a subcommand");
    }
    let sub = &args.args[0];
    bail!("unknown subcommand: '{sub}'");
}
