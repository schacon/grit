//! `gust for-each-ref` command stub.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `gust for-each-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `gust for-each-ref`.
pub fn run(_args: Args) -> Result<()> {
    bail!("`gust for-each-ref` is not implemented yet")
}
