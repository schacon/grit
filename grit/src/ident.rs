//! CLI wrappers for Git-compatible author/committer identity resolution.

use anyhow::Result;
use grit_lib::config::ConfigSet;
use grit_lib::ident_resolve::{IdentityError, SystemIdentityEnv};

pub use grit_lib::ident_config::ident_default_name;
pub use grit_lib::ident_resolve::{GitIdentityNameEnv, IdentRole};

fn system_env() -> SystemIdentityEnv {
    SystemIdentityEnv
}

/// Read a `GIT_*_NAME` variable like Git's `getenv`: unset vs set, preserving explicit empty.
#[must_use]
pub fn read_git_identity_name_env(key: &str) -> GitIdentityNameEnv {
    grit_lib::ident_resolve::read_git_identity_name_env_with(&system_env(), key)
}

/// Read `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` from the environment.
#[must_use]
pub fn read_git_identity_name_from_env(key: &str) -> Option<String> {
    grit_lib::ident_resolve::read_git_identity_name_from_env_with(&system_env(), key)
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

fn map_identity_error(err: IdentityError) -> anyhow::Error {
    match &err {
        IdentityError::AutoDetectionDisabled { role } => {
            eprintln!("{}", role.missing_email_hint());
        }
        IdentityError::EmptyName { role, .. } => {
            ident_env_hint(*role);
        }
        IdentityError::InvalidName { .. } => {}
    }
    anyhow::anyhow!(err.to_string())
}

/// Resolve email for a role when creating commits (honors `user.useConfigOnly`).
pub fn resolve_email(config: &ConfigSet, role: IdentRole) -> Result<String> {
    grit_lib::ident_resolve::resolve_email_with(&system_env(), config, role)
        .map_err(map_identity_error)
}

/// Resolve email without failing on `user.useConfigOnly` (e.g. `git var -l`, reflog-style).
#[must_use]
pub fn resolve_email_lenient(config: &ConfigSet, role: IdentRole) -> String {
    grit_lib::ident_resolve::resolve_email_lenient_with(&system_env(), config, role)
}

/// Name from env and config without erroring (for `git var -l`).
#[must_use]
pub fn peek_name(config: &ConfigSet, role: IdentRole) -> Option<String> {
    grit_lib::ident_resolve::peek_name_with(&system_env(), config, role)
}

/// Resolve name for a role when creating commits.
pub fn resolve_name(config: &ConfigSet, role: IdentRole) -> Result<String> {
    grit_lib::ident_resolve::resolve_name_with(&system_env(), config, role).map_err(|err| {
        if matches!(err, IdentityError::InvalidName { .. }) {
            return anyhow::anyhow!(err.to_string());
        }
        map_identity_error(err)
    })
}

/// Committer name/email for reflog and other non-strict contexts: never errors; always has an email.
pub fn resolve_loose_committer_parts(config: &ConfigSet) -> (String, String) {
    grit_lib::ident_resolve::resolve_loose_committer_parts_with(&system_env(), config)
}
