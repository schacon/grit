//! Git-compatible author/committer identity resolution (see upstream `ident.c`).
//!
//! Handles `user.useConfigOnly`, per-role `author.*` / `committer.*` overrides,
//! and the `EMAIL` environment fallback when auto-detection is allowed.

use anyhow::{bail, Result};
use grit_lib::config::ConfigSet;

/// Read `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` from the environment.
///
/// Shell scripts may `export` these with ISO-8859-1 bytes (see upstream `t3901`).
/// Rust's [`std::env::var`] rejects non-UTF-8 sequences; Git accepts arbitrary bytes
/// in the name field and stores them according to `i18n.commitEncoding`. We decode
/// invalid UTF-8 as Latin-1 to match Git's behavior for test fixtures.
#[must_use]
pub fn read_git_identity_name_from_env(key: &str) -> Option<String> {
    let os = std::env::var_os(key)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = os.as_bytes();
        let s = if std::str::from_utf8(bytes).is_ok() {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            crate::git_commit_encoding::decode_bytes(Some("ISO8859-1"), bytes)
        };
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_owned())
        }
    }
    #[cfg(not(unix))]
    {
        os.to_str()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }
}

/// Author vs committer for `GIT_*` / `author.*` / `committer.*` lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentRole {
    Author,
    Committer,
}

impl IdentRole {
    fn env_name_key(self) -> &'static str {
        match self {
            IdentRole::Author => "GIT_AUTHOR_NAME",
            IdentRole::Committer => "GIT_COMMITTER_NAME",
        }
    }

    fn env_email_key(self) -> &'static str {
        match self {
            IdentRole::Author => "GIT_AUTHOR_EMAIL",
            IdentRole::Committer => "GIT_COMMITTER_EMAIL",
        }
    }

    fn config_name_key(self) -> &'static str {
        match self {
            IdentRole::Author => "author.name",
            IdentRole::Committer => "committer.name",
        }
    }

    fn config_email_key(self) -> &'static str {
        match self {
            IdentRole::Author => "author.email",
            IdentRole::Committer => "committer.email",
        }
    }

    fn missing_email_hint(self) -> &'static str {
        match self {
            IdentRole::Author => "Author identity unknown",
            IdentRole::Committer => "Committer identity unknown",
        }
    }
}

fn use_config_only(config: &ConfigSet) -> bool {
    match config.get_bool("user.useConfigOnly") {
        Some(Ok(b)) => b,
        Some(Err(_)) => false,
        None => false,
    }
}

/// True if any of `user.email`, `author.email`, or `committer.email` is set to a non-empty value.
fn config_mail_given(config: &ConfigSet) -> bool {
    for key in ["user.email", "author.email", "committer.email"] {
        if let Some(v) = config.get(key) {
            if !v.trim().is_empty() {
                return true;
            }
        }
    }
    false
}

fn synthetic_email() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_owned());
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned());
    let domain = if host.contains('.') {
        host
    } else {
        format!("{host}.(none)")
    };
    format!("{user}@{domain}")
}

fn resolve_email_inner(
    config: &ConfigSet,
    role: IdentRole,
    honor_use_config_only: bool,
) -> Result<String> {
    if let Ok(v) = std::env::var(role.env_email_key()) {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_owned());
        }
    }

    if let Some(v) = config.get(role.config_email_key()) {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_owned());
        }
    }

    if let Some(v) = config.get("user.email") {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_owned());
        }
    }

    if honor_use_config_only && use_config_only(config) && !config_mail_given(config) {
        eprintln!("{}", role.missing_email_hint());
        bail!(
            "no email was given and auto-detection is disabled\n\n\
*** Please tell me who you are.\n\n\
Run\n\n\
  git config --global user.email \"you@example.com\"\n\
  git config --global user.name \"Your Name\"\n\n\
to set your account's default identity.\n\
Omit --global to set the identity only in this repository.\n"
        );
    }

    if let Ok(v) = std::env::var("EMAIL") {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_owned());
        }
    }

    Ok(synthetic_email())
}

/// Resolve email for a role when creating commits (honors `user.useConfigOnly`).
pub fn resolve_email(config: &ConfigSet, role: IdentRole) -> Result<String> {
    resolve_email_inner(config, role, true)
}

/// Resolve email without failing on `user.useConfigOnly` (e.g. `git var -l`, reflog-style).
#[must_use]
pub fn resolve_email_lenient(config: &ConfigSet, role: IdentRole) -> String {
    resolve_email_inner(config, role, false).unwrap_or_else(|_| synthetic_email())
}

/// Name from env and config without erroring (for `git var -l`).
#[must_use]
pub fn peek_name(config: &ConfigSet, role: IdentRole) -> Option<String> {
    if let Some(v) = read_git_identity_name_from_env(role.env_name_key()) {
        return Some(v);
    }
    if let Some(v) = config.get(role.config_name_key()) {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_owned());
        }
    }
    config
        .get("user.name")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Resolve name for a role when creating commits.
pub fn resolve_name(config: &ConfigSet, role: IdentRole) -> Result<String> {
    if let Some(n) = peek_name(config, role) {
        return Ok(n);
    }

    match role {
        IdentRole::Author => {
            eprintln!("Author identity unknown");
            bail!(
                "Author identity unknown\n\n\
Please tell me who you are.\n\n\
Run\n\n\
  git config user.email \"you@example.com\"\n\
  git config user.name \"Your Name\""
            );
        }
        IdentRole::Committer => Ok("Unknown".to_owned()),
    }
}

/// Committer name/email for reflog and other non-strict contexts: never errors; always has an email.
pub fn resolve_loose_committer_parts(config: &ConfigSet) -> (String, String) {
    let name = read_git_identity_name_from_env("GIT_COMMITTER_NAME")
        .or_else(|| read_git_identity_name_from_env("GIT_AUTHOR_NAME"))
        .or_else(|| {
            config
                .get("committer.name")
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            config
                .get("user.name")
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "Unknown".to_owned());

    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("GIT_AUTHOR_EMAIL")
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            config
                .get("committer.email")
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            config
                .get("user.email")
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var("EMAIL")
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(synthetic_email);

    (name, email)
}
