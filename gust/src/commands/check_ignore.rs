//! `gust check-ignore` command stub.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `gust check-ignore`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `gust check-ignore`.
pub fn run(_args: Args) -> Result<()> {
    bail!("`gust check-ignore` is not implemented yet")
}
