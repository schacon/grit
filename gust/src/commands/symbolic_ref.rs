//! `gust symbolic-ref` command stub.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `gust symbolic-ref`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `gust symbolic-ref`.
pub fn run(_args: Args) -> Result<()> {
    bail!("`gust symbolic-ref` is not implemented yet")
}
