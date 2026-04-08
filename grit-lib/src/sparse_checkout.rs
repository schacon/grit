//! Sparse-checkout pattern matching shared across commands.
//!
//! Matches Git's `path_in_sparse_checkout` behaviour: a path is "in" the sparse
//! checkout if it or any parent directory prefix matches the active patterns.
//!
//! In **cone mode**, Git writes an expanded gitignore-style pattern file
//! (`/*`, `!/*/`, `/A/`, `!/A/*/`, …). When that format is present, matching must
//! use the non-cone pattern engine even if `core.sparseCheckoutCone` is true.

use std::collections::BTreeSet;

use crate::wildmatch::{wildmatch, WM_PATHNAME};

/// Read non-empty, non-comment lines from `.git/info/sparse-checkout`.
pub fn parse_sparse_checkout_file(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// Returns true when the sparse-checkout file uses Git's expanded cone format
/// (starts with `/*` then `!/*/`).
pub fn sparse_checkout_lines_look_like_expanded_cone(lines: &[String]) -> bool {
    lines.len() >= 2 && lines[0] == "/*" && lines[1] == "!/*/"
}

/// Parent and recursive directory prefixes (no leading slash, no trailing slash) from an
/// expanded cone sparse-checkout file, matching Git `write_cone_to_file` layout.
fn parse_expanded_cone_parent_recursive(lines: &[String]) -> Option<(Vec<String>, Vec<String>)> {
    if !sparse_checkout_lines_look_like_expanded_cone(lines) {
        return None;
    }
    let mut parents = Vec::new();
    let mut recursive = Vec::new();
    let mut i = 2usize;
    while i + 1 < lines.len() {
        let a = &lines[i];
        let b = &lines[i + 1];
        if !a.starts_with('/') || !a.ends_with('/') || !b.starts_with('!') {
            break;
        }
        let inner_a = a.trim_start_matches('/').trim_end_matches('/');
        let expected_neg = format!("!/{inner_a}/*/");
        if b != &expected_neg {
            break;
        }
        parents.push(inner_a.to_string());
        i += 2;
    }
    while i < lines.len() {
        let line = &lines[i];
        if line.starts_with('!') {
            return None;
        }
        if !line.starts_with('/') || !line.ends_with('/') {
            return None;
        }
        let body = line.trim_start_matches('/').trim_end_matches('/');
        if body.is_empty() {
            return None;
        }
        recursive.push(body.to_string());
        i += 1;
    }
    Some((parents, recursive))
}

fn path_in_expanded_cone(path: &str, lines: &[String]) -> bool {
    let Some((parents, recursive)) = parse_expanded_cone_parent_recursive(lines) else {
        return false;
    };
    let path = path.trim_start_matches('/').trim_end_matches('/');

    if !path.contains('/') {
        return true;
    }

    for r in &recursive {
        if path == *r || path.starts_with(&format!("{r}/")) {
            return true;
        }
    }

    for p in &parents {
        let p_slash = format!("{}/", p);
        if path == *p {
            return true;
        }
        if !path.starts_with(&p_slash) {
            continue;
        }
        let rest = &path[p_slash.len()..];
        let Some(slash_pos) = rest.find('/') else {
            // Immediate child `p/name`: in-cone only when it leads into a recursive directory
            // (e.g. `sub/dir` under parent `sub`), not for unrelated files like `sub/d`.
            let combined = format!("{}/{}", p, rest);
            return recursive
                .iter()
                .any(|r| r == &combined || r.starts_with(&format!("{combined}/")));
        };
        let first = &rest[..slash_pos];
        let combined = format!("{}/{}", p, first);
        for r in &recursive {
            let under_r = path == *r || path.starts_with(&format!("{r}/"));
            let r_covers = r == &combined || r.starts_with(&format!("{combined}/"));
            if r_covers && under_r {
                return true;
            }
        }
    }

    false
}

/// Cone mode from config combined with on-disk pattern shape.
///
/// Git parses the sparse-checkout file in cone mode only when it matches the
/// expanded template (`/*`, `!/*/`, …). Raw lines like `a` are matched as
/// non-cone patterns even if `core.sparseCheckoutCone` is true.
#[must_use]
pub fn effective_cone_mode_for_sparse_file(cone_config: bool, lines: &[String]) -> bool {
    cone_config && sparse_checkout_lines_look_like_expanded_cone(lines)
}

/// Build the on-disk sparse-checkout contents for cone mode, matching
/// `write_cone_to_file` in Git's `builtin/sparse-checkout.c`.
///
/// `dirs` are worktree-relative directory paths as the user typed them (no
/// leading slash, `/` separators). Empty entries are ignored.
pub fn build_expanded_cone_sparse_checkout_lines(dirs: &[String]) -> Vec<String> {
    let mut recursive: BTreeSet<String> = BTreeSet::new();
    for d in dirs {
        let t = d.trim().trim_start_matches('/').trim_end_matches('/');
        if t.is_empty() {
            continue;
        }
        recursive.insert(format!("/{t}"));
    }

    let mut parents: BTreeSet<String> = BTreeSet::new();
    for r in &recursive {
        let mut cur = r.clone();
        loop {
            let Some(slash) = cur.rfind('/') else {
                break;
            };
            if slash == 0 {
                break;
            }
            cur.truncate(slash);
            parents.insert(cur.clone());
        }
    }

    let mut out = vec!["/*".to_owned(), "!/*/".to_owned()];

    for p in parents.iter() {
        if recursive.contains(p) {
            continue;
        }
        if recursive_set_has_strict_ancestor(&recursive, p) {
            continue;
        }
        let esc = escape_cone_pattern_path(p);
        out.push(format!("{esc}/"));
        out.push(format!("!{esc}/*/"));
    }

    for r in recursive.iter() {
        if recursive_set_has_strict_ancestor(&recursive, r) {
            continue;
        }
        let esc = escape_cone_pattern_path(r);
        out.push(format!("{esc}/"));
    }

    out
}

fn escape_cone_pattern_path(path_with_leading_slash: &str) -> String {
    // Git's `escaped_pattern` escapes backslashes, `[`, `*`, `?`, `#`; keep
    // tests (and normal paths) working with a minimal escape pass.
    let mut out = String::with_capacity(path_with_leading_slash.len() + 8);
    for ch in path_with_leading_slash.chars() {
        match ch {
            '\\' | '[' | '*' | '?' | '#' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn recursive_set_has_strict_ancestor(recursive: &BTreeSet<String>, path: &str) -> bool {
    let mut cur = path.to_string();
    loop {
        let Some(slash) = cur.rfind('/') else {
            break;
        };
        if slash == 0 {
            break;
        }
        cur.truncate(slash);
        if recursive.contains(&cur) {
            return true;
        }
    }
    false
}

/// Parse recursive directory paths from an expanded cone sparse-checkout file
/// (for merging on `sparse-checkout add`).
pub fn parse_expanded_cone_recursive_dirs(lines: &[String]) -> Vec<String> {
    if !sparse_checkout_lines_look_like_expanded_cone(lines) {
        return Vec::new();
    }
    let mut i = 2usize;
    let mut out = Vec::new();
    while i < lines.len() {
        let line = &lines[i];
        if line.starts_with('!') {
            i += 1;
            continue;
        }
        if !line.ends_with('/') || !line.starts_with('/') {
            i += 1;
            continue;
        }
        let trimmed = line.trim_end_matches('/');
        let body = trimmed.trim_start_matches('/');
        let esc = escape_cone_pattern_path(trimmed);
        let expected_neg = format!("!{esc}/*/");
        if i + 1 < lines.len() && lines[i + 1] == expected_neg {
            i += 2;
            continue;
        }
        out.push(body.to_owned());
        i += 1;
    }
    out
}

/// Returns true when `path` is included in the sparse-checkout definition.
///
/// Implements parent-directory fallback like Git's `path_in_sparse_checkout`:
/// if the full path does not match, successively shorter prefixes (directory
/// parents) are tried until one matches or the path is exhausted.
///
/// `path` must use `/` separators and be relative to the repository root.
pub fn path_in_sparse_checkout(path: &str, patterns: &[String], cone_mode: bool) -> bool {
    if path.is_empty() || patterns.is_empty() {
        return true;
    }

    // Git's expanded cone file uses parent + recursive directory rules, not plain gitignore
    // wildmatch on each line (see `write_cone_to_file` / `path_matches_pattern_list`).
    if sparse_checkout_lines_look_like_expanded_cone(patterns) {
        return path_in_expanded_cone(path, patterns);
    }

    // Prefix-directory rules apply to **raw** cone patterns on disk (e.g. `sub`).
    let use_cone_prefix = cone_mode;

    let mut end = path.len();
    while end > 0 {
        if path_matches_sparse_patterns(&path[..end], patterns, use_cone_prefix) {
            return true;
        }
        let Some(slash) = path[..end].rfind('/') else {
            break;
        };
        end = slash;
    }
    false
}

/// Like [`path_in_sparse_checkout`], but only applies when `cone_enabled` is true.
///
/// When sparse-checkout is not in cone mode, Git treats every path as "in" for
/// this check (backward compatibility for file destinations).
pub fn path_in_cone_mode_sparse_checkout(
    path: &str,
    patterns: &[String],
    cone_enabled: bool,
) -> bool {
    if !cone_enabled || patterns.is_empty() {
        return true;
    }
    path_in_sparse_checkout(path, patterns, true)
}

/// Returns true when `path` is included, using the same rules as
/// `grit sparse-checkout` / `apply_sparse_patterns`.
pub fn path_matches_sparse_patterns(path: &str, patterns: &[String], cone_mode: bool) -> bool {
    let expanded_cone = sparse_checkout_lines_look_like_expanded_cone(patterns);
    if expanded_cone {
        return path_in_expanded_cone(path, patterns);
    }
    // Raw cone mode (`sparse-checkout set --cone sub` writing only `sub`): directory-prefix rules.
    // Expanded on-disk cone (`/*`, `!/*/`, `/sub/`, …): use full pattern matching like Git.
    let raw_cone_prefix = cone_mode && !expanded_cone;

    if raw_cone_prefix {
        if !path.contains('/') {
            return true;
        }

        for pattern in patterns {
            let prefix = pattern.trim_end_matches('/');
            if path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/') {
                return true;
            }
            if path == prefix {
                return true;
            }
        }
        return false;
    }

    let mut included = false;
    for raw_pattern in patterns {
        let pattern = raw_pattern.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }

        let (negated, core_pattern) = if let Some(rest) = pattern.strip_prefix('!') {
            (true, rest)
        } else {
            (false, pattern)
        };
        if core_pattern.is_empty() || core_pattern == "/" {
            continue;
        }

        let matches = if let Some(prefix_with_slash) = core_pattern.strip_suffix('/') {
            // Directory-only patterns: `/a/` or `a/`.
            let inner = prefix_with_slash.trim_start_matches('/');
            if inner.is_empty() {
                false
            } else if negated && core_pattern == "/*/" {
                // Cone expanded form: after `/*` includes all top-level names, `!/*/` removes
                // nested paths (two+ segments). Single-segment paths like `a` stay included.
                let trimmed = path.trim_end_matches('/');
                trimmed.contains('/')
            } else if inner.contains('*') || inner.contains('?') || inner.contains('[') {
                // e.g. `!/sub/*/` in expanded cone mode
                let pat = format!("{prefix_with_slash}/");
                let text = format!("/{path}/");
                wildmatch(pat.as_bytes(), text.as_bytes(), WM_PATHNAME)
            } else {
                path == inner || path.starts_with(&format!("{inner}/"))
            }
        } else if core_pattern.starts_with('/') {
            // Leading `/` anchors to repo root (same as gitignore / sparse-checkout).
            let text = format!("/{}", path.trim_start_matches('/'));
            wildmatch(core_pattern.as_bytes(), text.as_bytes(), WM_PATHNAME)
        } else {
            wildmatch(core_pattern.as_bytes(), path.as_bytes(), WM_PATHNAME)
        };

        if matches {
            included = !negated;
        }
    }

    included
}
