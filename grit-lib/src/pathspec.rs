//! Git-compatible pathspec matching (magic tokens and global flags).
//!
//! Global flags are read from the same environment variables as Git:
//! `GIT_LITERAL_PATHSPECS`, `GIT_GLOB_PATHSPECS`, `GIT_NOGLOB_PATHSPECS`,
//! `GIT_ICASE_PATHSPECS`. The `grit` binary sets these from CLI flags such as
//! `--literal-pathspecs` before dispatching subcommands.

use std::borrow::Cow;

use crate::crlf::path_has_gitattribute;
use crate::crlf::AttrRule;
use crate::precompose_config::pathspec_precompose_enabled;
use crate::unicode_normalization::precompose_utf8_path;
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
    /// `:(top)` / short `:/` — paths are relative to repo root.
    top: bool,
    prefix: Option<String>,
    /// `:(attr:NAME)` — match paths that have gitattribute `NAME` set.
    attr_name: Option<String>,
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

/// Whether `GIT_LITERAL_PATHSPECS` is enabled (shell `*` and `?` are literal, not globs).
#[must_use]
pub fn literal_pathspecs_enabled() -> bool {
    literal_global()
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
        if let Some(name) = token.strip_prefix("attr:") {
            if !name.is_empty() {
                magic.attr_name = Some(name.to_string());
            }
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
        } else if token.eq_ignore_ascii_case("top") {
            magic.top = true;
        }
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
            b'/' => {
                magic.top = true;
                true
            } // short `:/` = top
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

/// Path prefix used for Bloom-filter lookups (`revision.c` `convert_pathspec_to_bloom_keyvec`).
///
/// `cwd_from_repo_root` is the path from the repository work tree to the process cwd, using `/`
/// separators and no leading slash (empty string at repo root). Used for `:(top)` / `:/`.
#[must_use]
pub fn bloom_lookup_prefix_with_cwd(
    spec: &str,
    cwd_from_repo_root: Option<&str>,
) -> Option<String> {
    let (elem_magic, raw_pattern) = parse_element_magic(spec);
    let magic = combine_magic(elem_magic);
    if magic.exclude || magic.icase {
        return None;
    }
    let pattern = strip_top_magic(raw_pattern);
    if pattern.is_empty() {
        return None;
    }
    let combined = if magic.top {
        let cwd = cwd_from_repo_root.unwrap_or("").trim_end_matches('/');
        if cwd.is_empty() {
            pattern.to_string()
        } else {
            format!("{cwd}/{pattern}")
        }
    } else {
        pattern.to_string()
    };
    let pattern = combined.as_str();
    let mut len = simple_length(pattern);
    if len != pattern.len() {
        while len > 0 && pattern.as_bytes()[len - 1] != b'/' {
            len -= 1;
        }
    }
    while len > 0 && pattern.as_bytes()[len - 1] == b'/' {
        len -= 1;
    }
    if len == 0 {
        return None;
    }
    Some(combined[..len].to_string())
}

#[must_use]
pub fn bloom_lookup_prefix(spec: &str) -> Option<String> {
    bloom_lookup_prefix_with_cwd(spec, None)
}

/// Whether every pathspec can participate in Bloom precomputation (Git `forbid_bloom_filters`).
#[must_use]
pub fn pathspecs_allow_bloom(specs: &[String]) -> bool {
    specs
        .iter()
        .all(|s| !s.is_empty() && bloom_lookup_prefix_with_cwd(s, None).is_some())
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

/// Optional path metadata for literal pathspecs with a trailing `/` (tree-walk / diff-tree).
///
/// Git treats `dir/` as “directory or git submodule only”: a regular file `dir`
/// does not match, but a tree entry `dir` or gitlink `dir` does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathspecMatchContext {
    /// The index/tree entry is a directory (mode `040000`).
    pub is_directory: bool,
    /// The entry is a git submodule / gitlink (`160000`).
    pub is_git_submodule: bool,
}

/// Returns whether `path` matches the pathspec `spec` with default (file) context.
///
/// For pathspecs ending in `/`, a path equal to the prefix matches only when
/// [`PathspecMatchContext`] indicates a directory or submodule; see
/// [`matches_pathspec_with_context`].
#[must_use]
pub fn matches_pathspec(spec: &str, path: &str) -> bool {
    matches_pathspec_with_context(spec, path, PathspecMatchContext::default())
}

/// Like [`matches_pathspec`], but uses `ctx` for trailing-`/` literal pathspecs.
#[must_use]
pub fn matches_pathspec_with_context(spec: &str, path: &str, ctx: PathspecMatchContext) -> bool {
    let spec_nfc: Cow<'_, str> = if pathspec_precompose_enabled() {
        precompose_utf8_path(spec)
    } else {
        Cow::Borrowed(spec)
    };
    let path_nfc: Cow<'_, str> = if pathspec_precompose_enabled() {
        precompose_utf8_path(path)
    } else {
        Cow::Borrowed(path)
    };
    let spec = spec_nfc.as_ref();
    let path = path_nfc.as_ref();

    let trimmed = spec.strip_prefix("./").unwrap_or(spec);
    if trimmed == "." || trimmed.is_empty() {
        return true;
    }

    if trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[') {
        let flags = if trimmed.contains("**") {
            WM_PATHNAME
        } else {
            0
        };
        if wildmatch(trimmed.as_bytes(), path.as_bytes(), flags) {
            return true;
        }
        if (ctx.is_directory || ctx.is_git_submodule)
            && !path.is_empty()
            && trimmed.len() > path.len()
            && trimmed.as_bytes().get(path.len()) == Some(&b'/')
            && trimmed.starts_with(path)
        {
            return true;
        }
        return false;
    }

    if let Some(prefix) = trimmed.strip_suffix('/') {
        if path.starts_with(&format!("{prefix}/")) {
            return true;
        }
        if path == prefix {
            return ctx.is_directory || ctx.is_git_submodule;
        }
        return false;
    }

    path == trimmed || path.starts_with(&format!("{trimmed}/"))
}

/// Parse a Git mode string (e.g. `100644`, `040000`) into a [`PathspecMatchContext`].
#[must_use]
pub fn context_from_mode_octal(mode: &str) -> PathspecMatchContext {
    let Ok(bits) = u32::from_str_radix(mode, 8) else {
        return PathspecMatchContext::default();
    };
    context_from_mode_bits(bits)
}

/// Classify a raw Git mode (e.g. from an index or tree entry) for pathspec matching.
#[must_use]
pub fn context_from_mode_bits(mode: u32) -> PathspecMatchContext {
    let ty = mode & 0o170000;
    PathspecMatchContext {
        is_directory: ty == 0o040000,
        is_git_submodule: ty == 0o160000,
    }
}

/// Match a pathspec against a tree path, using `.gitattributes` for `:(attr:...)`.
///
/// Used by `git archive` style tree walks: `mode` supplies directory/gitlink context for
/// literal pathspecs ending in `/`.
#[must_use]
pub fn matches_pathspec_for_object(
    spec: &str,
    path: &str,
    mode: u32,
    attr_rules: &[AttrRule],
) -> bool {
    let (elem_magic, raw_pattern) = parse_element_magic(spec);
    let magic = combine_magic(elem_magic);

    if magic.literal && magic.glob {
        return false;
    }
    if magic.exclude {
        return false;
    }

    let ctx = context_from_mode_bits(mode);
    let is_dir_for_attr = path.ends_with('/') || ctx.is_directory || ctx.is_git_submodule;

    if let Some(ref attr) = magic.attr_name {
        if !path_has_gitattribute(attr_rules, path, is_dir_for_attr, attr) {
            return false;
        }
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
    if magic.literal || magic.glob || magic.icase {
        pathspec_matches_tail(pattern, path_for_match, magic)
    } else {
        matches_pathspec_with_context(pattern, path_for_match, ctx)
    }
}

/// Returns wildmatch flags for `:(icase)` / `:(glob)`-style patterns when those
/// appear as explicit magic (not used by default CLI pathspecs).
#[must_use]
pub fn wildmatch_flags_icase_glob(icase: bool, glob: bool) -> u32 {
    let mut f = if glob { WM_PATHNAME } else { 0 };
    if icase {
        f |= WM_CASEFOLD;
    }
    f
}

#[cfg(test)]
mod tree_entry_pathspec_tests {
    use super::*;

    #[test]
    fn literal_prefix_and_exact() {
        assert!(matches_pathspec("path1", "path1/file1"));
        assert!(matches_pathspec_with_context(
            "path1/",
            "path1/file1",
            PathspecMatchContext::default()
        ));
        assert!(matches_pathspec("file0", "file0"));
        assert!(!matches_pathspec("path", "path1/file1"));
    }

    #[test]
    fn wildcards_cross_slash_by_default() {
        assert!(matches_pathspec("f*", "file0"));
        assert!(matches_pathspec("*file1", "path1/file1"));
        assert!(matches_pathspec_with_context(
            "path1/f*",
            "path1",
            PathspecMatchContext {
                is_directory: true,
                ..Default::default()
            }
        ));
        assert!(matches_pathspec("path1/*file1", "path1/file1"));
    }

    #[test]
    fn trailing_slash_directory_only() {
        assert!(!matches_pathspec_with_context(
            "file0/",
            "file0",
            PathspecMatchContext::default()
        ));
        assert!(matches_pathspec_with_context(
            "file0/",
            "file0",
            PathspecMatchContext {
                is_directory: true,
                ..Default::default()
            }
        ));
        assert!(matches_pathspec_with_context(
            "submod/",
            "submod",
            PathspecMatchContext {
                is_git_submodule: true,
                ..Default::default()
            }
        ));
    }
}
