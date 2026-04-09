//! `.gitmodules` validation (Git `fsck` / `submodule-config` parity).
//!
//! Submodule `path` and `url` values must not look like command-line options
//! (non-empty and starting with `-`). See Git's `looks_like_command_line_option` in `path.c`.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use crate::config::{ConfigFile, ConfigScope};
use crate::error::Result;
use crate::objects::{parse_commit, parse_tree, ObjectId, ObjectKind, TreeEntry};
use crate::odb::Odb;
use crate::pack::read_pack_index;

/// Returns `true` when `s` is non-empty and starts with `-` (Git `looks_like_command_line_option`).
#[must_use]
pub fn looks_like_command_line_option(s: &str) -> bool {
    !s.is_empty() && s.as_bytes().first() == Some(&b'-')
}

/// True when `name` names a `.gitmodules` file (HFS / NTFS spellings), not a symlink.
#[must_use]
pub fn tree_entry_is_gitmodules_blob(mode: u32, name: &[u8]) -> bool {
    if mode == 0o120000 {
        return false;
    }
    let Ok(name_str) = std::str::from_utf8(name) else {
        return false;
    };
    is_hfs_dot_gitmodules(name_str) || is_ntfs_dot_gitmodules(name_str)
}

fn next_hfs_char(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<char> {
    loop {
        let ch = chars.next()?;
        match ch {
            '\u{200c}' | '\u{200d}' | '\u{200e}' | '\u{200f}' => continue,
            '\u{202a}'..='\u{202e}' => continue,
            '\u{206a}'..='\u{206f}' => continue,
            '\u{feff}' => continue,
            _ => return Some(ch),
        }
    }
}

fn is_hfs_dot_generic(path: &str, needle: &str) -> bool {
    let mut chars = path.chars().peekable();
    let mut c = match next_hfs_char(&mut chars) {
        Some(x) => x,
        None => return false,
    };
    if c != '.' {
        return false;
    }
    for nc in needle.chars() {
        c = match next_hfs_char(&mut chars) {
            Some(x) => x,
            None => return false,
        };
        if c as u32 > 127 {
            return false;
        }
        if !c.eq_ignore_ascii_case(&nc) {
            return false;
        }
    }
    match next_hfs_char(&mut chars) {
        None => true,
        Some(ch) if ch == '/' => true,
        Some(_) => false,
    }
}

fn is_hfs_dot_gitmodules(path: &str) -> bool {
    is_hfs_dot_generic(path, "gitmodules")
}

fn only_spaces_and_periods(name: &str, mut i: usize) -> bool {
    let b = name.as_bytes();
    loop {
        let c = *b.get(i).unwrap_or(&0);
        if c == 0 || c == b':' {
            return true;
        }
        if c != b' ' && c != b'.' {
            return false;
        }
        i += 1;
    }
}

fn is_ntfs_dot_generic(name: &str, dotgit_name: &str, short_prefix: &str) -> bool {
    let b = name.as_bytes();
    let len = dotgit_name.len();
    if !b.is_empty()
        && b[0] == b'.'
        && name.len() > len
        && name[1..1 + len].eq_ignore_ascii_case(dotgit_name)
    {
        let i = len + 1;
        return only_spaces_and_periods(name, i);
    }

    if b.len() >= 8
        && name[..6].eq_ignore_ascii_case(&dotgit_name[..6])
        && b[6] == b'~'
        && (b[7] >= b'1' && b[7] <= b'4')
    {
        return only_spaces_and_periods(name, 8);
    }

    let mut i = 0usize;
    let mut saw_tilde = false;
    while i < 8 {
        let c = *b.get(i).unwrap_or(&0);
        if c == 0 {
            return false;
        }
        if saw_tilde {
            if !c.is_ascii_digit() {
                return false;
            }
        } else if c == b'~' {
            i += 1;
            let d = *b.get(i).unwrap_or(&0);
            if !(b'1'..=b'9').contains(&d) {
                return false;
            }
            saw_tilde = true;
        } else if i >= 6 {
            return false;
        } else if c & 0x80 != 0 {
            return false;
        } else {
            let sc = short_prefix.as_bytes().get(i).copied().unwrap_or(0);
            if (c as char).to_ascii_lowercase() != sc as char {
                return false;
            }
        }
        i += 1;
    }
    only_spaces_and_periods(name, i)
}

fn is_ntfs_dot_gitmodules(name: &str) -> bool {
    is_ntfs_dot_generic(name, "gitmodules", "gi7eba")
}

/// Write Git-style warnings for submodule path/url values that look like CLI options.
pub fn write_gitmodules_cli_option_warnings(
    w: &mut dyn Write,
    content: &str,
) -> std::io::Result<()> {
    if let Ok(config) = ConfigFile::parse(Path::new(".gitmodules"), content, ConfigScope::Local) {
        let mut any = false;
        for entry in &config.entries {
            let key = &entry.key;
            let Some(rest) = key.strip_prefix("submodule.") else {
                continue;
            };
            let Some(last_dot) = rest.rfind('.') else {
                continue;
            };
            let var = &rest[last_dot + 1..];
            if var != "path" && var != "url" {
                continue;
            }
            let Some(value) = entry.value.as_deref() else {
                continue;
            };
            if looks_like_command_line_option(value) {
                writeln!(
                    w,
                    "warning: ignoring '{key}' which may be interpreted as a command-line option: {value}"
                )?;
                any = true;
            }
        }
        if any {
            return Ok(());
        }
    }

    // Fallback: raw scan (handles minimal `.gitmodules` that the strict parser rejects).
    let mut subsection: Option<&str> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            subsection = None;
            if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                let inner = inner.trim();
                if let Some(rest) = inner.strip_prefix("submodule") {
                    let rest = rest.trim();
                    let name = rest
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(rest);
                    if !name.is_empty() {
                        subsection = Some(name);
                    }
                }
            }
            continue;
        }
        let Some((raw_key, raw_val)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key != "path" && key != "url" {
            continue;
        }
        let mut val = raw_val.trim();
        if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
            val = &val[1..val.len() - 1];
        }
        if looks_like_command_line_option(val) {
            let key_full = match subsection {
                Some(name) => format!("submodule.{name}.{key}"),
                None => key.to_string(),
            };
            writeln!(
                w,
                "warning: ignoring '{key_full}' which may be interpreted as a command-line option: {val}"
            )?;
        }
    }
    Ok(())
}

fn check_submodule_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let b = name.as_bytes();
    // Git `check_submodule_name`: `goto in_component` before the loop — first component.
    if b.len() >= 2 && b[0] == b'.' && b[1] == b'.'
        && (b.len() == 2 || b[2] == b'/' || b[2] == b'\\') {
            return false;
        }
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        i += 1;
        if c == b'/' || c == b'\\' {
            let j = i;
            if b.len() >= j + 2
                && b[j] == b'.'
                && b[j + 1] == b'.'
                && (j + 2 >= b.len() || b[j + 2] == b'/' || b[j + 2] == b'\\')
            {
                return false;
            }
        }
    }
    true
}

/// `true` when `value` is a command-style submodule update (`!…`), matching Git fsck.
fn submodule_update_is_command(value: &str) -> bool {
    !value.is_empty() && value.starts_with('!')
}

/// Validate a `.gitmodules` blob (Git `fsck_gitmodules_fn`). Returns `object hex: msg` or `None`.
pub fn validate_gitmodules_blob_line(data: &[u8]) -> Option<String> {
    let Ok(text) = std::str::from_utf8(data) else {
        return None;
    };
    let config = ConfigFile::parse(Path::new(".gitmodules"), text, ConfigScope::Local).ok()?;

    let mut worst: Option<String> = None;

    for entry in &config.entries {
        let key = &entry.key;
        let Some(rest) = key.strip_prefix("submodule.") else {
            continue;
        };
        let Some(last_dot) = rest.rfind('.') else {
            continue;
        };
        let name = &rest[..last_dot];
        let var = &rest[last_dot + 1..];

        if !check_submodule_name(name) {
            worst.get_or_insert_with(|| {
                format!("gitmodulesName: disallowed submodule name: {name}")
            });
        }

        let Some(value) = entry.value.as_deref() else {
            continue;
        };

        match var {
            "url" => {
                if looks_like_command_line_option(value) {
                    worst.get_or_insert_with(|| {
                        format!("gitmodulesUrl: disallowed submodule url: {value}")
                    });
                }
            }
            "path" => {
                if looks_like_command_line_option(value) {
                    worst = Some(format!(
                        "gitmodulesPath: disallowed submodule path: {value}"
                    ));
                }
            }
            "update" => {
                if submodule_update_is_command(value) {
                    worst.get_or_insert_with(|| {
                        format!("gitmodulesUpdate: disallowed submodule update setting: {value}")
                    });
                }
            }
            _ => {}
        }
    }

    worst
}

fn collect_gitmodules_blobs_from_tree(
    odb: &Odb,
    tree_oid: ObjectId,
    seen_trees: &mut HashSet<ObjectId>,
) -> Result<HashSet<ObjectId>> {
    let mut blobs = HashSet::new();
    let mut stack = vec![tree_oid];
    while let Some(tid) = stack.pop() {
        if !seen_trees.insert(tid) {
            continue;
        }
        let obj = odb.read(&tid)?;
        if obj.kind != ObjectKind::Tree {
            continue;
        }
        let entries = parse_tree(&obj.data)?;
        for TreeEntry { mode, name, oid } in entries {
            if tree_entry_is_gitmodules_blob(mode, &name) {
                blobs.insert(oid);
            } else if mode == 0o040000 {
                stack.push(oid);
            }
        }
    }
    Ok(blobs)
}

/// Validate every `.gitmodules` blob reachable from `commit_oid`. Returns `Some(hex: msg)` on error.
pub fn verify_gitmodules_for_commit(odb: &Odb, commit_oid: ObjectId) -> Result<Option<String>> {
    let obj = odb.read(&commit_oid)?;
    if obj.kind != ObjectKind::Commit {
        return Ok(None);
    }
    let commit = parse_commit(&obj.data)?;
    let mut seen_trees = HashSet::new();
    let blobs = collect_gitmodules_blobs_from_tree(odb, commit.tree, &mut seen_trees)?;
    for oid in blobs {
        let blob = odb.read(&oid)?;
        if blob.kind != ObjectKind::Blob {
            continue;
        }
        if let Some(msg) = validate_gitmodules_blob_line(&blob.data) {
            return Ok(Some(format!("{}: {}", oid.to_hex(), msg)));
        }
    }
    Ok(None)
}

/// Parse `objects/ab/cdef…` loose paths into OIDs; for `.idx` files load all contained OIDs.
pub fn oids_from_copied_object_paths(copied: &[PathBuf]) -> Result<HashSet<ObjectId>> {
    let mut out = HashSet::new();
    for p in copied {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".idx") {
            let idx = read_pack_index(p)?;
            for e in &idx.entries {
                out.insert(e.oid);
            }
            continue;
        }
        if let Some(oid) = object_id_from_loose_object_path(p) {
            out.insert(oid);
        }
    }
    Ok(out)
}

fn object_id_from_loose_object_path(path: &Path) -> Option<ObjectId> {
    let file_name = path.file_name()?.to_str()?;
    if file_name.len() != 38 {
        return None;
    }
    let parent = path.parent()?.file_name()?.to_str()?;
    if parent.len() != 2 {
        return None;
    }
    let hex = format!("{parent}{file_name}");
    ObjectId::from_hex(&hex).ok()
}
