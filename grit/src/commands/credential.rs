//! `grit credential` — retrieve and store user credentials.
//!
//! Implements the Git credential helper protocol:
//! - `fill`    — read credential spec from stdin, output filled credentials
//! - `approve` — mark credentials as good (no-op pass-through)
//! - `reject`  — mark credentials as bad (no-op pass-through)
//!
//! Reads key=value pairs (protocol, host, username, password, path) from
//! stdin and passes them through the configured credential helpers.

use anyhow::{bail, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

/// Arguments for `grit credential`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub action: CredentialAction,
}

#[derive(Debug, Subcommand)]
pub enum CredentialAction {
    /// Read credential spec from stdin, output filled credentials.
    Fill,
    /// Mark credentials as good.
    Approve,
    /// Mark credentials as bad.
    Reject,
}

/// Parse credential key=value pairs from stdin until a blank line or EOF.
fn read_credential_input() -> Result<BTreeMap<String, String>> {
    let stdin = io::stdin();
    let mut map = BTreeMap::new();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Ok(map)
}

/// Run `grit credential`.
pub fn run(args: Args) -> Result<()> {
    let creds = read_credential_input()?;

    match args.action {
        CredentialAction::Fill => {
            // For fill, we need at minimum protocol and host.
            if !creds.contains_key("protocol") || !creds.contains_key("host") {
                bail!("credential input must include protocol and host");
            }
            // Output all known fields back (pass-through).
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for (key, value) in &creds {
                writeln!(out, "{key}={value}")?;
            }
            writeln!(out)?;
        }
        CredentialAction::Approve => {
            // No-op: in a full implementation this would notify helpers
            // that the credentials were accepted.
        }
        CredentialAction::Reject => {
            // No-op: in a full implementation this would notify helpers
            // that the credentials were rejected and should be erased.
        }
    }

    Ok(())
}
