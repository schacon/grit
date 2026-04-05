//! `grit version` — print version string.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Write};

/// Arguments for `grit version`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display version information")]
pub struct Args {
    /// Show build options.
    #[arg(long = "build-options")]
    pub build_options: bool,
}

/// Run the `version` command.
pub fn run(args: Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "git version 2.47.0.grit")?;
    if args.build_options {
        writeln!(out, "sizeof-long: {}", std::mem::size_of::<i64>())?;
        writeln!(out, "sizeof-size_t: {}", std::mem::size_of::<usize>())?;
        writeln!(out, "shell-path: /bin/sh")?;
        writeln!(out, "default-hash: sha1")?;
    }
    Ok(())
}
