//! Git-compatible author/committer identity resolution (see upstream `ident.c`).
//!
//! Handles `user.useConfigOnly`, per-role `author.*` / `committer.*` overrides,
//! and the `EMAIL` environment fallback when auto-detection is allowed.

use anyhow::{bail, Result};
use grit_lib::config::ConfigSet;

pub use grit_lib::ident_config::ident_default_name;

/// Whether `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` is unset vs set (possibly empty).
///
/// Git treats a set-but-empty value as an explicit override: it must not fall through
/// to `user.name` or passwd/GECOS fallback (`t7518-ident-corner-cases`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitIdentityNameEnv {
    /// Variable is not present in the environment.
    Unset,
    /// Present after trimming whitespace (may be `""`).
    Set(String),
}

/// Read a `GIT_*_NAME` variable like Git's `getenv`: unset vs set, preserving explicit empty.
#[must_use]
pub fn read_git_identity_name_env(key: &str) -> GitIdentityNameEnv {
    let Some(os) = std::env::var_os(key) else {
        return GitIdentityNameEnv::Unset;
    };
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = os.as_bytes();
        let s = if std::str::from_utf8(bytes).is_ok() {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            crate::git_commit_encoding::decode_bytes(Some("ISO8859-1"), bytes)
        };
        GitIdentityNameEnv::Set(s.trim().to_owned())
    }
    #[cfg(not(unix))]
    {
        let s = os.to_str().map(|t| t.trim().to_owned()).unwrap_or_default();
        GitIdentityNameEnv::Set(s)
    }
}

/// Read `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` from the environment.
///
/// Shell scripts may `export` these with ISO-8859-1 bytes (see upstream `t3901`).
/// Rust's [`std::env::var`] rejects non-UTF-8 sequences; Git accepts arbitrary bytes
/// in the name field and stores them according to `i18n.commitEncoding`. We decode
/// invalid UTF-8 as Latin-1 to match Git's behavior for test fixtures.
///
/// Returns [`None`] when the variable is unset or set to whitespace only. A set-but-empty
/// value (after trim) is still [`None`] here; use [`read_git_identity_name_env`] when the
/// distinction matters.
#[must_use]
pub fn read_git_identity_name_from_env(key: &str) -> Option<String> {
    match read_git_identity_name_env(key) {
        GitIdentityNameEnv::Unset => None,
        GitIdentityNameEnv::Set(s) if s.is_empty() => None,
        GitIdentityNameEnv::Set(s) => Some(s),
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

fn ident_env_hint(role: IdentRole) {
    eprintln!("{}", role.missing_email_hint());
    eprintln!(
        "\n*** Please tell me who you are.\n\n\
Run\n\n\
  git config --global user.email \"you@example.com\"\n\
  git config --global user.name \"Your Name\"\n\n\
to set your account's default identity.\n\
Omit --global to set the identity only in this repository.\n"
    );
}

/// True if `name` contains at least one character Git does not treat as `crud`
/// (`ident.c` `has_non_crud`).
fn ident_name_has_non_crud(name: &str) -> bool {
    name.chars().any(|c| {
        let o = c as u32;
        !(o <= 32
            || c == ','
            || c == ':'
            || c == ';'
            || c == '<'
            || c == '>'
            || c == '"'
            || c == '\\'
            || c == '\'')
    })
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
    match read_git_identity_name_env(role.env_name_key()) {
        GitIdentityNameEnv::Set(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        GitIdentityNameEnv::Unset => {
            if let Some(v) = config.get(role.config_name_key()) {
                let t = v.trim();
                if !t.is_empty() {
                    return Some(t.to_owned());
                }
            }
            let d = ident_default_name(config);
            if d.is_empty() {
                None
            } else {
                Some(d)
            }
        }
    }
}

/// Resolve name for a role when creating commits.
pub fn resolve_name(config: &ConfigSet, role: IdentRole) -> Result<String> {
    let email = resolve_email_inner(config, role, true)?;

    let name: String = match read_git_identity_name_env(role.env_name_key()) {
        GitIdentityNameEnv::Set(s) => s,
        GitIdentityNameEnv::Unset => {
            if let Some(v) = config.get(role.config_name_key()) {
                let t = v.trim();
                if !t.is_empty() {
                    t.to_owned()
                } else {
                    ident_default_name(config)
                }
            } else {
                ident_default_name(config)
            }
        }
    };

    if name.is_empty() {
        ident_env_hint(role);
        bail!("empty ident name (for <{email}>) not allowed");
    }

    if !ident_name_has_non_crud(&name) {
        bail!("invalid ident name: '{}'", name);
    }

    Ok(name)
}

/// Committer name/email for reflog and other non-strict contexts: never errors; always has an email.
pub fn resolve_loose_committer_parts(config: &ConfigSet) -> (String, String) {
    let name = match read_git_identity_name_env("GIT_COMMITTER_NAME") {
        GitIdentityNameEnv::Set(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        GitIdentityNameEnv::Unset => read_git_identity_name_from_env("GIT_AUTHOR_NAME"),
    }
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
    .or_else(|| {
        let d = ident_default_name(config);
        if d.is_empty() {
            None
        } else {
            Some(d)
        }
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
