//! `grit credential-store` — store credentials on disk.
//!
//! File-based credential storage in `~/.git-credentials`.
//! Supports the credential helper protocol actions: `get`, `store`, `erase`.
//!
//! Credentials are stored as URL lines: `protocol://user:password@host/path`

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// Arguments for `grit credential-store`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The credential-helper action: get, store, or erase.
    pub action: String,

    /// Path to the credentials file (default: ~/.git-credentials).
    #[arg(long)]
    pub file: Option<PathBuf>,
}

fn credentials_path(file: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = file {
        return Ok(p.clone());
    }
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".git-credentials"))
}

fn read_input() -> Result<BTreeMap<String, String>> {
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

/// Build a URL-style credential line from components.
fn to_url_line(creds: &BTreeMap<String, String>) -> Option<String> {
    let protocol = creds.get("protocol")?;
    let host = creds.get("host")?;
    let username = creds.get("username").map(|s| s.as_str()).unwrap_or("");
    let password = creds.get("password").map(|s| s.as_str()).unwrap_or("");
    let path = creds.get("path").map(|s| s.as_str()).unwrap_or("");

    let userinfo = if !username.is_empty() && !password.is_empty() {
        format!("{username}:{password}@")
    } else if !username.is_empty() {
        format!("{username}@")
    } else {
        String::new()
    };

    let suffix = if path.is_empty() {
        String::new()
    } else {
        format!("/{path}")
    };

    Some(format!("{protocol}://{userinfo}{host}{suffix}"))
}

/// Check if a stored URL line matches the query credentials.
fn line_matches(line: &str, creds: &BTreeMap<String, String>) -> bool {
    // Parse the URL line: protocol://[user[:pass]@]host[/path]
    let Some(rest) = line.strip_suffix('\n').or(Some(line)) else {
        return false;
    };
    let Some((protocol, rest)) = rest.split_once("://") else {
        return false;
    };

    if let Some(qp) = creds.get("protocol") {
        if qp != protocol {
            return false;
        }
    }

    let (_, host_path) = if let Some(at_pos) = rest.rfind('@') {
        rest.split_at(at_pos + 1)
    } else {
        ("", rest)
    };

    let host = host_path.split('/').next().unwrap_or(host_path);
    if let Some(qh) = creds.get("host") {
        if qh != host {
            return false;
        }
    }

    true
}

/// Run `grit credential-store`.
pub fn run(args: Args) -> Result<()> {
    let path = credentials_path(&args.file)?;

    match args.action.as_str() {
        "get" => {
            let creds = read_input()?;
            if !path.exists() {
                return Ok(());
            }
            let contents = fs::read_to_string(&path).context("reading credentials file")?;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for line in contents.lines() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line_matches(line, &creds) {
                    // Parse and output as key=value
                    if let Some(rest) = line.split_once("://").map(|(p, r)| {
                        writeln!(out, "protocol={p}").ok();
                        r
                    }) {
                        if let Some(at_pos) = rest.rfind('@') {
                            let userinfo = &rest[..at_pos];
                            let host_path = &rest[at_pos + 1..];
                            if let Some((user, pass)) = userinfo.split_once(':') {
                                writeln!(out, "username={user}").ok();
                                writeln!(out, "password={pass}").ok();
                            } else {
                                writeln!(out, "username={userinfo}").ok();
                            }
                            let host = host_path.split('/').next().unwrap_or(host_path);
                            writeln!(out, "host={host}").ok();
                        } else {
                            let host = rest.split('/').next().unwrap_or(rest);
                            writeln!(out, "host={host}").ok();
                        }
                    }
                    writeln!(out)?;
                    return Ok(());
                }
            }
        }
        "store" => {
            let creds = read_input()?;
            if let Some(url_line) = to_url_line(&creds) {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .context("opening credentials file")?;
                // Set restrictive permissions on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
                }
                writeln!(file, "{url_line}")?;
            }
        }
        "erase" => {
            let creds = read_input()?;
            if !path.exists() {
                return Ok(());
            }
            let contents = fs::read_to_string(&path).context("reading credentials file")?;
            let remaining: Vec<&str> = contents
                .lines()
                .filter(|line| !line_matches(line, &creds))
                .collect();
            fs::write(&path, remaining.join("\n") + "\n").context("writing credentials file")?;
        }
        other => bail!("unknown credential-store action: {other}"),
    }

    Ok(())
}
