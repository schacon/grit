//! `grit credential-cache` — cache credentials in memory.
//!
//! Accepts credential helper protocol actions (`get`, `store`, `erase`)
//! and delegates to an in-memory cache daemon.  This is a basic stub that
//! accepts the protocol but does not yet implement a persistent daemon.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use std::io::{self, BufRead, Write};

/// Arguments for `grit credential-cache`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The credential-helper action: get, store, or erase.
    pub action: String,

    /// Timeout in seconds for cached credentials (default: 900).
    #[arg(long, default_value_t = 900)]
    pub timeout: u64,

    /// Path to the cache daemon socket.
    #[arg(long)]
    pub socket: Option<String>,
}

/// Run `grit credential-cache`.
pub fn run(args: Args) -> Result<()> {
    match args.action.as_str() {
        "get" => {
            // Read credential spec from stdin, but we have no daemon yet,
            // so just consume input and output nothing (cache miss).
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let line = line?;
                if line.is_empty() {
                    break;
                }
            }
            // Output blank line to indicate no cached credential.
            let stdout = io::stdout();
            let mut out = stdout.lock();
            writeln!(out)?;
        }
        "store" => {
            // Read and discard — would store in daemon memory.
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let line = line?;
                if line.is_empty() {
                    break;
                }
            }
        }
        "erase" => {
            // Read and discard — would tell daemon to erase.
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let line = line?;
                if line.is_empty() {
                    break;
                }
            }
        }
        other => bail!("unknown credential-cache action: {other}"),
    }
    Ok(())
}
