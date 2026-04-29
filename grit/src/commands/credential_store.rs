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

fn home_credentials_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".git-credentials"))
}

fn xdg_credentials_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"));
    Ok(base.join("git").join("credentials"))
}

fn lookup_paths(file: &Option<PathBuf>) -> Result<Vec<PathBuf>> {
    if let Some(path) = file {
        return Ok(vec![path.clone()]);
    }
    Ok(vec![home_credentials_path()?, xdg_credentials_path()?])
}

fn store_path(file: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = file {
        return Ok(path.clone());
    }
    let paths = lookup_paths(file)?;
    Ok(paths
        .iter()
        .find(|path| path.exists())
        .cloned()
        .unwrap_or_else(|| paths[0].clone()))
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

#[derive(Clone, Debug)]
struct StoredCredential {
    protocol: String,
    host: String,
    username: Option<String>,
    password: Option<String>,
    path: Option<String>,
}

impl StoredCredential {
    fn parse(line: &str) -> Option<Self> {
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (protocol, rest) = line.split_once("://")?;
        if protocol.is_empty() {
            return None;
        }
        let (userinfo, host_path) = rest
            .rsplit_once('@')
            .map_or((None, rest), |(u, hp)| (Some(u), hp));
        let (host, path) = host_path
            .split_once('/')
            .map_or((host_path, None), |(h, p)| (h, Some(p)));
        if host.is_empty() || host.contains('\r') {
            return None;
        }
        let (username, password) = match userinfo {
            Some(userinfo) => match userinfo.split_once(':') {
                Some((user, pass)) => (Some(user.to_string()), Some(pass.to_string())),
                None => (Some(userinfo.to_string()), None),
            },
            None => (None, None),
        };
        if username.is_none() || password.is_none() {
            return None;
        }
        Some(Self {
            protocol: protocol.to_string(),
            host: host.to_string(),
            username,
            password,
            path: path.map(ToOwned::to_owned),
        })
    }

    fn matches_query(&self, query: &BTreeMap<String, String>, include_password: bool) -> bool {
        if query
            .get("protocol")
            .is_some_and(|protocol| protocol != &self.protocol)
        {
            return false;
        }
        if query.get("host").is_some_and(|host| host != &self.host) {
            return false;
        }
        if let Some(username) = query.get("username").filter(|value| !value.is_empty()) {
            if self.username.as_deref() != Some(username.as_str()) {
                return false;
            }
        }
        if include_password {
            if let Some(password) = query.get("password") {
                if self.password.as_deref() != Some(password.as_str()) {
                    return false;
                }
            }
        }
        if let Some(path) = query.get("path").filter(|value| !value.is_empty()) {
            if self.path.as_deref() != Some(path.as_str()) {
                return false;
            }
        }
        true
    }
}

fn credential_matches_line(
    line: &str,
    creds: &BTreeMap<String, String>,
    include_password: bool,
) -> bool {
    StoredCredential::parse(line)
        .is_some_and(|stored| stored.matches_query(creds, include_password))
}

fn output_stored_credential(
    out: &mut impl Write,
    stored: &StoredCredential,
    query: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    writeln!(out, "protocol={}", stored.protocol)?;
    writeln!(out, "host={}", stored.host)?;
    if query.get("path").is_some_and(|path| !path.is_empty()) {
        if let Some(path) = stored.path.as_deref() {
            writeln!(out, "path={path}")?;
        }
    }
    if let Some(username) = stored.username.as_deref() {
        writeln!(out, "username={username}")?;
    }
    if let Some(password) = stored.password.as_deref() {
        writeln!(out, "password={password}")?;
    }
    writeln!(out)
}

fn to_url_line(creds: &BTreeMap<String, String>) -> Option<String> {
    if creds
        .get("ephemeral")
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
    {
        return None;
    }
    let protocol = creds.get("protocol")?;
    let host = creds.get("host")?;
    let username = creds.get("username").map(|s| s.as_str()).unwrap_or("");
    let password = creds.get("password").map(|s| s.as_str()).unwrap_or("");
    let path = creds.get("path").map(|s| s.as_str()).unwrap_or("");

    let userinfo = if creds.contains_key("username") && creds.contains_key("password") {
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

fn read_credential_lines(path: &PathBuf) -> Result<Vec<String>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    Ok(String::from_utf8_lossy(&bytes)
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn write_credential_lines(path: &PathBuf, lines: &[String]) -> Result<()> {
    let mut data = String::new();
    if !lines.is_empty() {
        data.push_str(&lines.join("\n"));
        data.push('\n');
    }
    fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

/// Run `grit credential-store`.
pub fn run(args: Args) -> Result<()> {
    match args.action.as_str() {
        "get" => {
            let creds = read_input()?;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for path in lookup_paths(&args.file)? {
                for line in read_credential_lines(&path)? {
                    let Some(stored) = StoredCredential::parse(&line) else {
                        continue;
                    };
                    if stored.matches_query(&creds, false) {
                        output_stored_credential(&mut out, &stored, &creds)?;
                        return Ok(());
                    }
                }
            }
        }
        "store" => {
            let creds = read_input()?;
            let Some(url_line) = to_url_line(&creds) else {
                return Ok(());
            };
            let path = store_path(&args.file)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            let mut lines = read_credential_lines(&path)?
                .into_iter()
                .filter(|line| !credential_matches_line(line, &creds, false))
                .collect::<Vec<_>>();
            lines.push(url_line);
            write_credential_lines(&path, &lines)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
        }
        "erase" => {
            let creds = read_input()?;
            for path in lookup_paths(&args.file)? {
                if !path.exists() {
                    continue;
                }
                let lines = read_credential_lines(&path)?
                    .into_iter()
                    .filter(|line| !credential_matches_line(line, &creds, true))
                    .collect::<Vec<_>>();
                write_credential_lines(&path, &lines)?;
            }
        }
        other => bail!("unknown credential-store action: {other}"),
    }

    Ok(())
}
