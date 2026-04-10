//! Safety checks for local transport URLs (matches Git `connect.c` / `path.c`).

use anyhow::{bail, Result};

/// Returns true when `str` is non-empty and begins with `-`, matching Git's
/// `looks_like_command_line_option` (used before quoting a path for shell-backed transport).
#[must_use]
pub(crate) fn looks_like_command_line_option(s: &str) -> bool {
    !s.is_empty() && s.starts_with('-')
}

/// Rejects repository path strings that could be mistaken for options when passed to a shell.
///
/// Git dies with `strange pathname '%s' blocked` when the parsed local path starts with `-`.
/// Absolute paths like `/tmp/-repo.git` are allowed because the path string begins with `/`.
pub(crate) fn check_local_url_path_not_option_like(url: &str) -> Result<()> {
    let path = url
        .strip_prefix("file://")
        .unwrap_or(url)
        .split('?')
        .next()
        .unwrap_or("");
    if looks_like_command_line_option(path) {
        bail!("fatal: strange pathname '{path}' blocked");
    }
    Ok(())
}
