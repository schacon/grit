//! `grit multi-pack-index` — manage multi-pack index files.
//!
//! Subcommands: `write` (create MIDX), `verify` (check MIDX integrity),
//! `repack` (repack using MIDX), `compact` (compact incremental layers).
//!
//! The `--incremental` flag for `write` is accepted for compatibility.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit multi-pack-index`.
#[derive(Debug, ClapArgs)]
#[command(about = "Manage multi-pack index")]
pub struct Args {
    /// Raw arguments (reserved for native MIDX support).
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit multi-pack-index`.
pub fn run(_args: Args) -> Result<()> {
    bail!("not implemented: grit multi-pack-index")
}
