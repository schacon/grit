//! `grit version` — print version string.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Write};

/// Arguments for `grit version`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display version information")]
pub struct Args {}

/// Run the `version` command.
pub fn run(_args: Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "git version 2.47.0.grit")?;
    Ok(())
}
