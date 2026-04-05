//! `grit fast-import` — import from a fast-export stream.
//!
//! Reads a fast-import formatted stream from stdin and creates the
//! corresponding objects and refs.  Parses `commit`, `blob`, `reset`,
//! and `tag` directives.

use crate::commands::system_git;
use anyhow::Result;
use clap::Args as ClapArgs;

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments forwarded to the system Git binary.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-import` by delegating to the system Git binary.
pub fn run(args: Args) -> Result<()> {
    system_git::run("fast-import", &args.args)
}
