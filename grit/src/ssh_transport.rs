//! SSH URL parsing and local resolution for test harnesses (`GIT_SSH` wrappers).

use anyhow::{bail, Result};
use grit_lib::repo::Repository;
use std::path::{Path, PathBuf};

/// Parsed SSH remote (scp-style `host:path` or `ssh://` / `git+ssh://`).
#[derive(Debug, Clone)]
pub struct SshUrl {
    pub host: String,
    pub path: String,
    pub scp_style: bool,
}

/// True when `url` is an SSH transport address (not plain local path).
pub fn is_configured_ssh_url(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("ssh://") || u.starts_with("git+ssh://") || is_scp_style_ssh_url(u)
}

fn is_scp_style_ssh_url(u: &str) -> bool {
    if u.contains("://") {
        return false;
    }
    if let Some(colon) = u.find(':') {
        let host = &u[..colon];
        let path = &u[colon + 1..];
        !host.is_empty() && !path.is_empty()
    } else {
        false
    }
}

/// Parse and validate `url` as Git would for SSH.
pub fn parse_ssh_url(url: &str) -> Result<SshUrl> {
    let u = url.trim();
    if u.starts_with("git+ssh://") {
        return parse_ssh_url_form(&u["git+ssh://".len()..]);
    }
    if let Some(rest) = u.strip_prefix("ssh://") {
        return parse_ssh_url_form(rest);
    }
    parse_scp_style(u)
}

fn parse_ssh_url_form(rest: &str) -> Result<SshUrl> {
    let after_slashes = rest.strip_prefix("//").unwrap_or(rest);
    let (authority, path_part) = split_ssh_authority_and_path(after_slashes)?;
    let host = extract_host_from_authority(authority)?;
    if host.starts_with('-') {
        bail!("ssh: hostname starts with '-'");
    }
    let mut path = normalize_ssh_url_path(path_part)?;
    // `ssh://host/path` uses a path from the remote root; the first `/` after the
    // host is not retained in `path_part`, so restore an absolute path.
    if !path.starts_with('/') {
        path = format!("/{path}");
    }
    Ok(SshUrl {
        host,
        path,
        scp_style: false,
    })
}

fn split_ssh_authority_and_path(s: &str) -> Result<(&str, &str)> {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => return Ok((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    Ok((s, ""))
}

fn extract_host_from_authority(authority: &str) -> Result<String> {
    let auth = authority.rsplit('@').next().unwrap_or(authority);
    let hostport = if let Some(rest) = auth.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("ssh: malformed host"))?;
        &rest[..end]
    } else {
        auth.split(':').next().unwrap_or(auth)
    };
    if hostport.is_empty() {
        bail!("ssh: empty host");
    }
    Ok(hostport.to_owned())
}

fn normalize_ssh_url_path(path_part: &str) -> Result<String> {
    let path = path_part.split('?').next().unwrap_or(path_part);
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        bail!("ssh: empty path");
    }
    let decoded = percent_decode_path(path)?;
    if decoded.starts_with('-') {
        bail!("ssh: path starts with '-'");
    }
    Ok(decoded)
}

fn percent_decode_path(path: &str) -> Result<String> {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars
                .next()
                .ok_or_else(|| anyhow::anyhow!("ssh: bad % escape"))?;
            let h2 = chars
                .next()
                .ok_or_else(|| anyhow::anyhow!("ssh: bad % escape"))?;
            let byte = u8::from_str_radix(&format!("{h1}{h2}"), 16)
                .map_err(|_| anyhow::anyhow!("ssh: bad % escape"))?;
            out.push(byte as char);
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn parse_scp_style(u: &str) -> Result<SshUrl> {
    let colon_pos = u
        .find(':')
        .ok_or_else(|| anyhow::anyhow!("ssh: no ':' in scp-style url"))?;
    let host = &u[..colon_pos];
    let path = &u[colon_pos + 1..];
    if host.is_empty() || path.is_empty() {
        bail!("ssh: empty host or path");
    }
    if host.starts_with('-') {
        bail!("ssh: hostname starts with '-'");
    }
    if path.starts_with('-') {
        bail!("ssh: path starts with '-'");
    }
    Ok(SshUrl {
        host: host.to_owned(),
        path: path.to_owned(),
        scp_style: true,
    })
}

/// Resolve `spec` to a local git directory when using a `GIT_SSH` wrapper or absolute paths.
pub fn try_local_git_dir(spec: &SshUrl) -> Option<PathBuf> {
    let path = Path::new(&spec.path);
    if path.is_absolute() {
        return resolve_git_dir_at(path);
    }
    if let Ok(trash) = std::env::var("TRASH_DIRECTORY") {
        let joined = PathBuf::from(trash).join(&spec.host).join(&spec.path);
        if let Some(gd) = resolve_git_dir_at(&joined) {
            return Some(gd);
        }
    }
    None
}

fn resolve_git_dir_at(path: &Path) -> Option<PathBuf> {
    if Repository::open(path, None).is_ok() {
        return Some(path.to_path_buf());
    }
    let git = path.join(".git");
    if Repository::open(&git, Some(path)).is_ok() {
        return Some(git);
    }
    None
}
