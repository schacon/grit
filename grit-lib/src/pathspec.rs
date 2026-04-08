//! Git-compatible pathspec matching (magic tokens and global flags).
//!
//! Global flags are read from the same environment variables as Git:
//! `GIT_LITERAL_PATHSPECS`, `GIT_GLOB_PATHSPECS`, `GIT_NOGLOB_PATHSPECS`,
//! `GIT_ICASE_PATHSPECS`. The `grit` binary sets these from CLI flags such as
//! `--literal-pathspecs` before dispatching subcommands.

use crate::wildmatch::{wildmatch, WM_CASEFOLD, WM_PATHNAME};

/// Returns the length of the leading literal segment before the first glob metacharacter,
/// matching Git's `simple_length()` (`*` `?` `[` `\`) on bytes.
#[must_use]
pub fn simple_length(match_str: &str) -> usize {
    let b = match_str.as_bytes();
    let mut len = 0usize;
    for &c in b {
        if matches!(c, b'*' | b'?' | b'[' | b'\\') {
            break;
        }
        len += 1;
    }
    len
}

#[derive(Debug, Clone, Default)]
struct PathspecMagic {
    literal: bool,
    glob: bool,
    icase: bool,
    exclude: bool,
    prefix: Option<String>,
}

fn parse_maybe_bool(v: &str) -> Option<bool> {
    let s = v.trim().to_ascii_lowercase();
    match s.as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn git_env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => parse_maybe_bool(&v).unwrap_or(default),
        Err(_) => default,
    }
}

fn literal_global() -> bool {
    git_env_bool("GIT_LITERAL_PATHSPECS", false)
}

fn glob_global() -> bool {
    git_env_bool("GIT_GLOB_PATHSPECS", false)
}

fn noglob_global() -> bool {
    git_env_bool("GIT_NOGLOB_PATHSPECS", false)
}

fn icase_global() -> bool {
    git_env_bool("GIT_ICASE_PATHSPECS", false)
}

/// Validates global pathspec environment flags the same way Git does.
///
/// Returns an error message suitable for `bail!` when flags are incompatible.
#[must_use]
pub fn validate_global_pathspec_flags() -> Result<(), String> {
    let lit = literal_global();
    let glob = glob_global();
    let noglob = noglob_global();
    let icase = icase_global();

    if glob && noglob {
        return Err("global 'glob' and 'noglob' pathspec settings are incompatible".to_string());
    }
    if lit && (glob || noglob || icase) {
        return Err(
            "global 'literal' pathspec setting is incompatible with all other global pathspec settings"
                .to_string(),
        );
    }
    Ok(())
}

fn parse_long_magic(rest_after_paren: &str) -> Option<(PathspecMagic, &str)> {
    let close = rest_after_paren.find(')')?;
    let magic_part = &rest_after_paren[..close];
    let tail = &rest_after_paren[close + 1..];
    let mut magic = PathspecMagic::default();
    for raw in magic_part.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(p) = token.strip_prefix("prefix:") {
            magic.prefix = Some(p.to_string());
            continue;
        }
        if token.eq_ignore_ascii_case("literal") {
            magic.literal = true;
        } else if token.eq_ignore_ascii_case("glob") {
            magic.glob = true;
        } else if token.eq_ignore_ascii_case("icase") {
            magic.icase = true;
        } else if token.eq_ignore_ascii_case("exclude") {
            magic.exclude = true;
        }
        // Ignore unknown tokens (attr:, top, etc.) for matching purposes.
    }
    Some((magic, tail))
}

/// `elem` is the full pathspec beginning with `:` (short magic form, not `:(...)`).
fn parse_short_magic(elem: &str) -> (PathspecMagic, &str) {
    let bytes = elem.as_bytes();
    let mut i = 1usize;
    let mut magic = PathspecMagic::default();
    while i < bytes.len() && bytes[i] != b':' {
        let ch = bytes[i];
        if ch == b'^' {
            magic.exclude = true;
            i += 1;
            continue;
        }
        let is_magic = match ch {
            b'!' => {
                magic.exclude = true;
                true
            }
            b'/' => true, // :(top) — strip `:/` from pattern later
            _ => false,
        };
        if is_magic {
            i += 1;
            continue;
        }
        break;
    }
    if i < bytes.len() && bytes[i] == b':' {
        i += 1;
    }
    (magic, &elem[i..])
}

/// Strip `:(magic)` / `:magic` prefix when not in literal-global mode.
fn parse_element_magic(elem: &str) -> (PathspecMagic, &str) {
    if !elem.starts_with(':') || literal_global() {
        return (PathspecMagic::default(), elem);
    }
    if elem.starts_with(":(") {
        return parse_long_magic(&elem[2..]).unwrap_or((PathspecMagic::default(), elem));
    }
    parse_short_magic(elem)
}

fn combine_magic(element: PathspecMagic) -> PathspecMagic {
    let mut m = element;
    if literal_global() {
        m.literal = true;
    }
    if glob_global() && !m.literal {
        m.glob = true;
    }
    if icase_global() {
        m.icase = true;
    }
    if noglob_global() && !m.glob {
        m.literal = true;
    }
    m
}

fn strip_top_magic(mut pattern: &str) -> &str {
    if let Some(r) = pattern.strip_prefix(":/") {
        pattern = r;
    }
    pattern
}

/// True if `path` is matched by `spec` (Git pathspec syntax, including magic and globals).
#[must_use]
pub fn pathspec_matches(spec: &str, path: &str) -> bool {
    let (elem_magic, raw_pattern) = parse_element_magic(spec);
    let magic = combine_magic(elem_magic);

    if magic.literal && magic.glob {
        // Git dies; treat as non-match for robustness.
        return false;
    }

    if magic.exclude {
        // Exclude pathspecs are handled by higher layers; do not match positively here.
        return false;
    }

    let pattern = strip_top_magic(raw_pattern);
    let path_for_match = if let Some(prefix) = magic.prefix.as_deref() {
        if !path.starts_with(prefix) {
            return false;
        }
        &path[prefix.len()..]
    } else {
        path
    };

    pathspec_matches_tail(pattern, path_for_match, magic)
}

fn pathspec_matches_tail(pattern: &str, path: &str, magic: PathspecMagic) -> bool {
    if pattern.is_empty() {
        return true;
    }

    let flags = if magic.icase { WM_CASEFOLD } else { 0 };

    if magic.literal {
        return literal_prefix_match(pattern, path);
    }

    let wm_flags = if magic.glob {
        flags | WM_PATHNAME
    } else {
        flags
    };

    let pattern_bytes = pattern.as_bytes();
    let path_bytes = path.as_bytes();
    let simple = simple_length(pattern);

    if simple < pattern.len() {
        if wildmatch(pattern_bytes, path_bytes, wm_flags) {
            return true;
        }
    } else if ps_str_eq(pattern, path, magic.icase) {
        return true;
    }

    if let Some(prefix) = pattern.strip_suffix('/') {
        if ps_str_eq(prefix, path, magic.icase) {
            return true;
        }
        let prefix_slash = format!("{prefix}/");
        if path_starts_with(path, &prefix_slash, magic.icase) {
            return true;
        }
        return false;
    }

    let prefix_slash = format!("{pattern}/");
    path == pattern || path_starts_with(path, &prefix_slash, magic.icase)
}

fn ps_str_eq(a: &str, b: &str, icase: bool) -> bool {
    if icase {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn path_starts_with(path: &str, prefix: &str, icase: bool) -> bool {
    if icase {
        path.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    } else {
        path.starts_with(prefix)
    }
}

fn literal_prefix_match(pattern: &str, path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('/') {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    path == pattern || path.starts_with(&format!("{pattern}/"))
}
