//! `grit gc` command.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;

/// Arguments for `grit gc`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Raw command arguments forwarded by the CLI parser.
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}

/// Run `grit gc`.
pub fn run(_args: Args) -> Result<()> {
    bail!("not implemented: grit gc")
}
