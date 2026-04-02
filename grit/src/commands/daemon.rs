//! `grit daemon` — Git protocol daemon.
//!
//! Starts a daemon that serves Git repositories over the git:// protocol.
//! Currently a stub that accepts common flags but reports that daemon
//! mode is not supported.
//!
//!     grit daemon [--base-path=<path>] [<directory>...]

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

/// Arguments for `grit daemon`.
#[derive(Debug, ClapArgs)]
#[command(about = "A really simple server for Git repositories")]
pub struct Args {
    /// Base path for all served repositories.
    #[arg(long = "base-path")]
    pub base_path: Option<PathBuf>,

    /// Listen on a specific port (default: 9418).
    #[arg(long)]
    pub port: Option<u16>,

    /// Export all repositories without needing git-daemon-export-ok.
    #[arg(long = "export-all")]
    pub export_all: bool,

    /// Run in inetd mode.
    #[arg(long)]
    pub inetd: bool,

    /// Enable verbose logging.
    #[arg(long)]
    pub verbose: bool,

    /// Directories to serve.
    #[arg(value_name = "DIRECTORY")]
    pub directories: Vec<PathBuf>,
}

/// Run `grit daemon`.
pub fn run(_args: Args) -> Result<()> {
    bail!("daemon mode is not yet supported in grit")
}
