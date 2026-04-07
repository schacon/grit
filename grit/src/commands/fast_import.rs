//! `grit fast-import` — import from a fast-export stream.
//!
//! Reads a fast-import formatted stream from stdin and creates the
//! corresponding objects and refs.  Parses `commit`, `blob`, `reset`,
//! and `tag` directives.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit fast-import`.
#[derive(Debug, ClapArgs)]
#[command(about = "Import from fast-export stream")]
pub struct Args {
    /// Raw arguments (reserved for native fast-import).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-import`.
pub fn run(_args: Args) -> Result<()> {
    bail!("not implemented: grit fast-import")
}
