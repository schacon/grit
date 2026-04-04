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

    /// Initial timeout in seconds (0 means no timeout).
    #[arg(long = "init-timeout")]
    pub init_timeout: Option<String>,

    /// Idle timeout in seconds (0 means no timeout).
    #[arg(long)]
    pub timeout: Option<String>,

    /// Maximum number of simultaneous connections.
    #[arg(long = "max-connections")]
    pub max_connections: Option<String>,

    /// Directories to serve.
    #[arg(value_name = "DIRECTORY")]
    pub directories: Vec<PathBuf>,
}

/// Validate that a string is a non-negative integer; die with git-compatible
/// `fatal:` message on failure.
fn validate_non_negative_int(value: &str, name: &str) {
    match value.parse::<i64>() {
        Ok(n) if n >= 0 => {}
        _ => {
            eprintln!(
                "fatal: invalid {} '{}', expecting a non-negative integer",
                name, value
            );
            std::process::exit(128);
        }
    }
}

/// Validate that a string is an integer (may be negative); die with
/// git-compatible `fatal:` message on failure.
fn validate_int(value: &str, name: &str) {
    match value.parse::<i64>() {
        Ok(_) => {}
        _ => {
            eprintln!(
                "fatal: invalid {} '{}', expecting an integer",
                name, value
            );
            std::process::exit(128);
        }
    }
}

/// Run `grit daemon`.
pub fn run(args: Args) -> Result<()> {
    // Validate numeric flags before doing anything else, matching git's
    // error messages exactly.
    if let Some(ref v) = args.init_timeout {
        validate_non_negative_int(v, "init-timeout");
    }
    if let Some(ref v) = args.timeout {
        validate_non_negative_int(v, "timeout");
    }
    if let Some(ref v) = args.max_connections {
        validate_int(v, "max-connections");
    }
    bail!("daemon mode is not yet supported in grit")
}
