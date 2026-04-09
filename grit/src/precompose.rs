//! Precompose UTF-8 path arguments (NFC) when `core.precomposeunicode` is enabled.
//!
//! Matches Git's `precompose_argv_prefix` for builtins that run after repository setup.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use grit_lib::config::{parse_bool, ConfigSet};
use grit_lib::repo::Repository;

fn precomposeunicode_enabled(git_dir: Option<&Path>) -> bool {
    let Ok(cfg) = (match git_dir {
        Some(p) => ConfigSet::load(Some(p), true),
        None => ConfigSet::load(None, true),
    }) else {
        return false;
    };
    cfg.get("core.precomposeunicode")
        .as_deref()
        .and_then(|v| parse_bool(v).ok())
        .unwrap_or(false)
}

/// Find `.git` by walking parents without opening the repository.
///
/// When [`Repository::discover`] fails (e.g. `safe.directory`), we still need the local
/// `core.precomposeunicode` value for argv normalization — same as Git after `setup_git_directory`.
fn locate_git_dir_filesystem_only() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git.canonicalize().unwrap_or(dot_git));
        }
        if dot_git.is_file() {
            let content = fs::read_to_string(&dot_git).ok()?;
            for line in content.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("gitdir:") {
                    let p = Path::new(rest.trim());
                    let resolved = if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        dir.join(p)
                    };
                    return Some(resolved.canonicalize().unwrap_or(resolved));
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn discover_git_dir() -> Option<PathBuf> {
    Repository::discover(None)
        .ok()
        .map(|r| r.git_dir)
        .or_else(locate_git_dir_filesystem_only)
}

/// Normalize path-like argv segments to NFC when `core.precomposeunicode` is true.
/// NFC-normalize pathspec arguments for plumbing commands (`diff-files`, `diff-index`, …).
///
/// Pathspecs follow `--`, or after the last option token when `--` is absent.
/// `diff-tree`: same positional split as `parse_options` — first two non-option args after `--`
/// are tree-ish; the rest are pathspecs. Without `--`, the same rule applies (no `end_of_options`).
pub(crate) fn precompose_diff_tree_argv(args: &mut [String]) {
    let mut end_of_options = false;
    let mut object_count = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if !end_of_options && arg == "--" {
            end_of_options = true;
            i += 1;
            continue;
        }
        if !end_of_options && arg.starts_with('-') && arg.as_str() != "-" {
            i += 1;
            continue;
        }
        let is_pathspec = end_of_options || object_count >= 2;
        if is_pathspec {
            let n = grit_lib::unicode_normalization::precompose_utf8_path(&args[i]).into_owned();
            if n != args[i] {
                args[i] = n;
            }
        } else {
            object_count += 1;
        }
        i += 1;
    }
}

pub(crate) fn precompose_plumbing_argv(args: &mut [String]) {
    if let Some(sep) = args.iter().position(|a| a == "--") {
        for a in args.iter_mut().skip(sep + 1) {
            let n = grit_lib::unicode_normalization::precompose_utf8_path(a).into_owned();
            if n != *a {
                *a = n;
            }
        }
        return;
    }
    let mut i = 0usize;
    while i < args.len() && args[i].starts_with('-') && args[i].as_str() != "-" {
        i += 1;
    }
    for a in args.iter_mut().skip(i) {
        let n = grit_lib::unicode_normalization::precompose_utf8_path(a).into_owned();
        if n != *a {
            *a = n;
        }
    }
}

pub(crate) fn precompose_dispatch_argv(subcmd: &str, rest: &mut [String]) {
    let enabled = if subcmd == "init" {
        precomposeunicode_enabled(None)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let gd = grit_lib::precompose_config::locate_git_dir_from_cwd(cwd);
        grit_lib::precompose_config::argv_precompose_enabled(gd.as_deref())
    };
    if !enabled {
        return;
    }
    // Git's `precompose_argv_prefix`: NFC-normalize every subcommand argument (not only paths).
    for a in rest.iter_mut() {
        *a = grit_lib::unicode_normalization::precompose_utf8_path(a).into_owned();
    }
}
