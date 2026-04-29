//! Safety checks for local transport URLs (matches Git `connect.c` / `path.c`).

use thiserror::Error;

/// Errors returned while validating local transport paths.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TransportPathError {
    /// A repository path begins with `-` and could be interpreted as a command option.
    #[error("fatal: strange pathname '{0}' blocked")]
    OptionLikePath(String),
}

/// Returns true when `s` is non-empty and begins with `-`, matching Git's
/// `looks_like_command_line_option` (used before quoting a path for shell-backed transport).
#[must_use]
pub fn looks_like_command_line_option(s: &str) -> bool {
    !s.is_empty() && s.starts_with('-')
}

/// Rejects repository path strings that could be mistaken for options when passed to a shell.
///
/// Git dies with `strange pathname '%s' blocked` when the parsed local path starts with `-`.
/// Absolute paths like `/tmp/-repo.git` are allowed because the path string begins with `/`.
pub fn check_local_url_path_not_option_like(url: &str) -> Result<(), TransportPathError> {
    let path = url
        .strip_prefix("file://")
        .unwrap_or(url)
        .split('?')
        .next()
        .unwrap_or("");
    if looks_like_command_line_option(path) {
        return Err(TransportPathError::OptionLikePath(path.to_owned()));
    }
    Ok(())
}
