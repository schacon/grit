//! Shared-repository permission helpers (`core.sharedRepository`, `--shared`).
//!
//! Mirrors Git's `git_config_perm`, `calc_shared_perm`, and `adjust_shared_perm` in `setup.c` /
//! `path.c`.

use crate::config::parse_bool;
use crate::diagnostics::{self, DiagnosticSink};
#[cfg(unix)]
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Use the process umask for new files (Git `PERM_UMASK`).
pub const PERM_UMASK: i32 = 0;
const OLD_PERM_GROUP: i32 = 1;
const OLD_PERM_EVERYBODY: i32 = 2;
/// Group-writable layout (`--shared=group` / `1`).
pub const PERM_GROUP: i32 = 0o660;
/// World-readable/writable layout (`--shared=all` / `2`).
pub const PERM_EVERYBODY: i32 = 0o664;

/// Parse `core.sharedRepository` / `git init --shared=` like Git's `git_config_perm`.
///
/// Returns an error when an octal mode is given but the owner lacks read+write (Git `die`).
pub fn git_config_perm(var: &str, value: &str) -> Result<i32, String> {
    git_config_perm_with_diagnostics(var, value, None)
}

/// Like [`git_config_perm`] but may emit [`diagnostics::Warning::BadBooleanConfig`].
pub fn git_config_perm_with_diagnostics(
    var: &str,
    value: &str,
    diagnostics: Option<&dyn DiagnosticSink>,
) -> Result<i32, String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("umask") {
        return Ok(PERM_UMASK);
    }
    if value.eq_ignore_ascii_case("group") {
        return Ok(PERM_GROUP);
    }
    if value.eq_ignore_ascii_case("all")
        || value.eq_ignore_ascii_case("world")
        || value.eq_ignore_ascii_case("everybody")
    {
        return Ok(PERM_EVERYBODY);
    }

    // Git: strtol(value, &endptr, 8); if *endptr != 0, fall through to bool.
    let (octal_prefix, rest) = split_octal_prefix(value);
    if rest.is_empty() && !octal_prefix.is_empty() {
        if let Ok(i) = i32::from_str_radix(octal_prefix, 8) {
            return Ok(match i {
                PERM_UMASK => PERM_UMASK,
                OLD_PERM_GROUP => PERM_GROUP,
                OLD_PERM_EVERYBODY => PERM_EVERYBODY,
                _ => {
                    if (i & 0o600) != 0o600 {
                        return Err(format!(
                            "problem with core.sharedRepository filemode value (0{i:o}).\n\
                             The owner of files must always have read and write permissions."
                        ));
                    }
                    -(i & 0o666)
                }
            });
        }
    }

    match parse_bool(value) {
        Ok(true) => Ok(PERM_GROUP),
        Ok(false) => Ok(PERM_UMASK),
        Err(_) => {
            if let Some(d) = diagnostics {
                d.warn(diagnostics::Warning::BadBooleanConfig {
                    key: var.to_owned(),
                    value: value.to_owned(),
                });
            }
            Ok(PERM_UMASK)
        }
    }
}

fn split_octal_prefix(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b'0'..=b'7') {
        i += 1;
    }
    (&s[..i], &s[i..])
}

/// Git's `calc_shared_perm` (`path.c`).
#[must_use]
pub fn calc_shared_perm(shared_repo: i32, mode: u32) -> u32 {
    let tweak = if shared_repo < 0 {
        (-shared_repo) as u32
    } else {
        shared_repo as u32
    };

    let mut new_mode = if shared_repo < 0 {
        (mode & !0o777) | tweak
    } else {
        mode | tweak
    };

    if mode & 0o200 == 0 {
        new_mode &= !0o222;
    }
    if mode & 0o100 != 0 {
        new_mode |= (new_mode & 0o444) >> 2;
    }

    new_mode
}

/// Git's `adjust_shared_perm` for a single path (`path.c`), using an explicit `shared_repo` value.
///
/// When `shared_repo` is zero, this is a no-op. Symlinks are skipped.
#[cfg(unix)]
pub fn adjust_shared_perm_path(shared_repo: i32, path: &Path) -> std::io::Result<()> {
    if shared_repo == 0 {
        return Ok(());
    }

    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }

    let old_mode = meta.permissions().mode();
    let mut new_mode = calc_shared_perm(shared_repo, old_mode);
    if meta.is_dir() {
        new_mode |= (new_mode & 0o444) >> 2;
        // Match Git: setgid on directories is tied to explicit filemode-style sharing
        // (`shared_repo < 0`), not the legacy positive `PERM_GROUP` / `PERM_EVERYBODY` tweaks.
        // Default `grit init` uses positive `PERM_GROUP` and should report `775` in `stat -c %a`
        // (t12660); `git init --shared=0660` uses a negative mask and needs `drwxrw[sx]---` (t1301).
        if shared_repo < 0 {
            const S_ISGID: u32 = 0o002000;
            if (new_mode & 0o060) != 0 {
                new_mode |= S_ISGID;
            }
        }
    }

    let new_perm = fs::Permissions::from_mode(new_mode & 0o7777);
    if (old_mode & 0o7777) != (new_mode & 0o7777) {
        fs::set_permissions(path, new_perm)?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn adjust_shared_perm_path(_shared_repo: i32, _path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Resolve `core.sharedRepository` from config text (merged set's string value).
pub fn shared_repository_from_config_value(raw: Option<&str>) -> Result<i32, String> {
    let Some(v) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(PERM_UMASK);
    };
    git_config_perm("core.sharedRepository", v)
}
