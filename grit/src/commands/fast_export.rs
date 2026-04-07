//! `grit fast-export` — export repository as a fast-import stream.
//!
//! Produces a stream on stdout suitable for `git fast-import`, walking
//! commits and emitting `commit`, `blob`, and `reset` directives.
//! Supports `--all` to export every ref.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit fast-export`.
#[derive(Debug, ClapArgs)]
#[command(about = "Export repository as fast-import stream")]
pub struct Args {
    /// Raw arguments (reserved for native fast-export).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit fast-export`.
pub fn run(_args: Args) -> Result<()> {
    bail!("not implemented: grit fast-export")
}
