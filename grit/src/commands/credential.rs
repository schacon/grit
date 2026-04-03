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
use std::process::{Command, Stdio};

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

/// Discover the `.git` directory by walking up from the current directory.
fn find_git_dir() -> Option<std::path::PathBuf> {
    // Check GIT_DIR env var first
    if let Ok(d) = std::env::var("GIT_DIR") {
        let p = std::path::PathBuf::from(&d);
        if p.is_dir() {
            return Some(p);
        }
    }
    // Walk up from cwd looking for .git
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            let dot_git = dir.join(".git");
            if dot_git.is_dir() {
                return Some(dot_git);
            }
            // Bare repo check
            if dir.join("HEAD").is_file() && dir.join("objects").is_dir() {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

/// Read `credential.helper` from Git config (supports -c overrides via
/// `GIT_CONFIG_PARAMETERS` and the normal config file cascade).
fn get_credential_helper() -> Option<String> {
    let git_dir = find_git_dir();
    let config = grit_lib::config::ConfigSet::load(
        git_dir.as_deref(),
        true,
    )
    .unwrap_or_default();
    config.get("credential.helper")
}

/// Invoke an external credential helper program.
///
/// The helper name from config (e.g. `test-helper`) is expanded to
/// `git-credential-test-helper`.  The helper is spawned with `get` as
/// its sole argument.  The credential fields are written to its stdin
/// followed by a blank line; its stdout is parsed for key=value pairs
/// that are merged back into the credential map.
fn invoke_helper(
    helper: &str,
    action: &str,
    creds: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let program = format!("git-credential-{helper}");

    let mut child = Command::new(&program)
        .arg(action)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to run credential helper '{program}': {e}"))?;

    // Write credential fields to helper's stdin, followed by blank line.
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for (key, value) in creds {
            writeln!(stdin, "{key}={value}")?;
        }
        writeln!(stdin)?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "credential helper '{program}' exited with status {}",
            output.status
        );
    }

    // Parse helper output — key=value lines until blank line or EOF.
    let mut result = creds.clone();
    for line in output.stdout.split(|&b| b == b'\n') {
        let line = std::str::from_utf8(line).unwrap_or("").trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            result.insert(key.to_string(), value.to_string());
        }
    }

    Ok(result)
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

            // Try to invoke configured credential helper.
            let filled = if let Some(helper) = get_credential_helper() {
                if !helper.is_empty() {
                    invoke_helper(&helper, "get", &creds)?
                } else {
                    creds
                }
            } else {
                creds
            };

            // Output all known fields.
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for (key, value) in &filled {
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
