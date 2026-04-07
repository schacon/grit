//! Pathspec matching utilities shared across commands.

use grit_lib::wildmatch::{wildmatch, WM_CASEFOLD};
use std::path::{Path, PathBuf};

/// True if `c` is glob-special in Git's pathspec rules (`*`, `?`, `[`, `\`).
#[inline]
fn is_glob_special_byte(c: u8) -> bool {
    matches!(c, b'*' | b'?' | b'[' | b'\\')
}

/// Length of the literal prefix before the first glob-special byte (Git's `simple_length`).
///
/// Backslash is glob-special: in `f\*` the prefix length is `1` (`f`), and the wildcard
/// tail `\*` is matched with [`wildmatch`]. A lone `\` at the end is treated as a literal
/// backslash (no glob tail).
#[must_use]
pub fn simple_length(pattern: &str) -> usize {
    let b = pattern.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if is_glob_special_byte(b[i]) {
            return i;
        }
        i += 1;
    }
    i
}

/// Whether the pattern uses wildcards after Git pathspec escaping rules.
#[must_use]
pub fn has_glob_chars(s: &str) -> bool {
    simple_length(s) < s.len()
}

/// Simple glob matching for git pathspecs.
/// `*` matches any sequence of characters including `/`.
/// `?` matches any single character except `/`.
/// `[abc]` matches any one character in the set.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == b'?' && text[ti] != b'/' {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            if let Some((matched, end)) = match_char_class(&pattern[pi..], text[ti]) {
                if matched {
                    pi += end;
                    ti += 1;
                } else if star_pi != usize::MAX {
                    star_ti += 1;
                    ti = star_ti;
                    pi = star_pi + 1;
                } else {
                    return false;
                }
            } else if star_pi != usize::MAX {
                star_ti += 1;
                ti = star_ti;
                pi = star_pi + 1;
            } else {
                return false;
            }
        } else if pi < pattern.len() && pattern[pi] == text[ti] {
            pi += 1;
            ti += 1;
        } else if star_pi != usize::MAX {
            star_ti += 1;
            ti = star_ti;
            pi = star_pi + 1;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

fn match_char_class(pattern: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pattern.is_empty() || pattern[0] != b'[' {
        return None;
    }
    let mut i = 1;
    let negate = i < pattern.len() && (pattern[i] == b'!' || pattern[i] == b'^');
    if negate {
        i += 1;
    }
    let mut matched = false;
    while i < pattern.len() && pattern[i] != b']' {
        if i + 2 < pattern.len() && pattern[i + 1] == b'-' {
            if ch >= pattern[i] && ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if ch == pattern[i] {
                matched = true;
            }
            i += 1;
        }
    }
    if i < pattern.len() && pattern[i] == b']' {
        if negate {
            matched = !matched;
        }
        Some((matched, i + 1))
    } else {
        None
    }
}

/// Check whether a path matches a pathspec (which may be literal or glob).
pub fn pathspec_matches(spec: &str, path: &str) -> bool {
    let (magic, mut pattern) = parse_magic(spec);
    if let Some(rest) = pattern.strip_prefix(":/") {
        pattern = rest;
    }

    let path = if let Some(prefix) = magic.prefix.as_deref() {
        if !path.starts_with(prefix) {
            return false;
        }
        &path[prefix.len()..]
    } else {
        path
    };

    if pattern.is_empty() {
        return true;
    }

    let wm_flags = if magic.icase { WM_CASEFOLD } else { 0 };
    let nwl = simple_length(pattern);

    if nwl == pattern.len() {
        if magic.icase {
            let pattern_folded = pattern.to_ascii_lowercase();
            let path_folded = path.to_ascii_lowercase();
            if let Some(prefix) = pattern_folded.strip_suffix('/') {
                path_folded == prefix || path_folded.starts_with(&format!("{prefix}/"))
            } else {
                path_folded == pattern_folded
                    || path_folded.starts_with(&format!("{pattern_folded}/"))
            }
        } else if let Some(prefix) = pattern.strip_suffix('/') {
            path == prefix || path.starts_with(&format!("{prefix}/"))
        } else {
            path == pattern || path.starts_with(&format!("{pattern}/"))
        }
    } else {
        let lit = &pattern.as_bytes()[..nwl];
        let path_b = path.as_bytes();
        if path_b.len() < nwl {
            return false;
        }
        let path_lit = &path_b[..nwl];
        let same = if magic.icase {
            path_lit.eq_ignore_ascii_case(lit)
        } else {
            path_lit == lit
        };
        if !same {
            return false;
        }
        let pat_rest = &pattern[nwl..];
        let path_rest = &path[nwl..];
        wildmatch(pat_rest.as_bytes(), path_rest.as_bytes(), wm_flags)
    }
}

/// Resolve a magic pathspec relative to a current-directory prefix.
///
/// This keeps the `cwd` prefix case-sensitive (via an internal `prefix:` magic
/// token) while still honoring magic options like `icase` for the tail.
/// Returns `None` when `spec` is not a parseable magic pathspec.
pub fn resolve_magic_pathspec(spec: &str, cwd_prefix: &str) -> Option<String> {
    if !spec.starts_with(":(") {
        return None;
    }
    let close_idx = spec.find(')')?;
    let magic_prefix = &spec[..=close_idx];
    let tail = &spec[close_idx + 1..];
    Some(resolve_magic_pathspec_parts(magic_prefix, tail, cwd_prefix))
}

#[derive(Debug, Default)]
struct PathspecMagic {
    icase: bool,
    prefix: Option<String>,
}

fn parse_magic(spec: &str) -> (PathspecMagic, &str) {
    let Some(rest) = spec.strip_prefix(":(") else {
        return (PathspecMagic::default(), spec);
    };
    let Some(close) = rest.find(')') else {
        return (PathspecMagic::default(), spec);
    };

    let (magic_part, tail_with_paren) = rest.split_at(close);
    let mut magic = PathspecMagic::default();
    for token in magic_part
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if token.eq_ignore_ascii_case("icase") {
            magic.icase = true;
        } else if let Some(prefix) = token.strip_prefix("prefix:") {
            magic.prefix = Some(prefix.to_string());
        }
    }

    (magic, &tail_with_paren[1..])
}

fn resolve_magic_pathspec_parts(magic_prefix: &str, tail: &str, cwd_prefix: &str) -> String {
    if has_magic_prefix_token(magic_prefix) {
        return format!("{magic_prefix}{tail}");
    }

    if let Some(rooted_tail) = tail.strip_prefix('/') {
        return format!("{magic_prefix}{}", normalize_relative_path_str(rooted_tail));
    }

    let combined = if cwd_prefix.is_empty() {
        normalize_relative_path_str(tail)
    } else {
        normalize_relative_path_str(&format!("{cwd_prefix}{tail}"))
    };

    let cwd_base = normalize_relative_path_str(cwd_prefix.trim_end_matches('/'));
    if !cwd_base.is_empty()
        && (combined == cwd_base || combined.starts_with(&format!("{cwd_base}/")))
    {
        let remainder = combined
            .strip_prefix(&cwd_base)
            .unwrap_or(combined.as_str())
            .strip_prefix('/')
            .unwrap_or(combined.as_str());
        let magic_with_prefix = inject_magic_prefix_token(magic_prefix, &format!("{cwd_base}/"));
        return format!("{magic_with_prefix}{remainder}");
    }

    format!("{magic_prefix}{combined}")
}

fn has_magic_prefix_token(magic_prefix: &str) -> bool {
    let Some(inner) = magic_prefix
        .strip_prefix(":(")
        .and_then(|s| s.strip_suffix(')'))
    else {
        return false;
    };
    inner
        .split(',')
        .map(str::trim)
        .any(|token| token.starts_with("prefix:"))
}

fn inject_magic_prefix_token(magic_prefix: &str, prefix: &str) -> String {
    let Some(inner) = magic_prefix
        .strip_prefix(":(")
        .and_then(|s| s.strip_suffix(')'))
    else {
        return magic_prefix.to_string();
    };
    if inner.trim().is_empty() {
        format!(":(prefix:{prefix})")
    } else {
        format!(":({inner},prefix:{prefix})")
    }
}

fn normalize_relative_path_str(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in std::path::Path::new(path).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(seg) => {
                parts.push(seg.to_string_lossy().to_string());
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

/// Current directory relative to `work_tree`, or `None` if cwd is the work tree root.
#[must_use]
pub fn pathdiff(cwd: &Path, work_tree: &Path) -> Option<String> {
    let cwd_canon = cwd.canonicalize().ok()?;
    let wt_canon = work_tree.canonicalize().ok()?;

    if cwd_canon == wt_canon {
        return None;
    }

    cwd_canon
        .strip_prefix(&wt_canon)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Resolve a pathspec string to a path relative to the repository work tree.
///
/// `prefix` is the current directory relative to the work tree (no trailing slash),
/// or `None` when cwd is the work tree root.
#[must_use]
pub fn resolve_pathspec(pathspec: &str, work_tree: &Path, prefix: Option<&str>) -> String {
    if pathspec == "." {
        return prefix.unwrap_or("").to_owned();
    }
    if pathspec.contains("../") || pathspec.starts_with("../") {
        let cwd = std::env::current_dir().unwrap_or_default();
        let abs = cwd.join(pathspec);
        let wt_canon = work_tree
            .canonicalize()
            .unwrap_or_else(|_| work_tree.to_path_buf());
        let mut parts: Vec<std::ffi::OsString> = Vec::new();
        for component in abs.components() {
            use std::path::Component;
            match component {
                Component::ParentDir => {
                    parts.pop();
                }
                Component::CurDir => {}
                other => parts.push(other.as_os_str().to_os_string()),
            }
        }
        let abs_norm: PathBuf = parts.iter().collect();
        if let Ok(rel) = abs_norm.strip_prefix(&wt_canon) {
            return rel.to_string_lossy().to_string();
        }
    }
    if Path::new(pathspec).is_absolute() {
        let abs = Path::new(pathspec);
        let wt_canon = work_tree
            .canonicalize()
            .unwrap_or_else(|_| work_tree.to_path_buf());
        let abs_canon = abs.canonicalize().unwrap_or_else(|_| abs.to_path_buf());
        if let Ok(rel) = abs_canon.strip_prefix(&wt_canon) {
            return rel.to_string_lossy().to_string();
        }
        return pathspec.to_owned();
    }

    if pathspec.starts_with(':') {
        if let Some(rest) = pathspec.strip_prefix(":/") {
            return rest.to_owned();
        }
        if pathspec.len() > 1 && pathspec.as_bytes()[1] == b'(' {
            if let Some(close) = pathspec[2..].find(')') {
                let pattern = &pathspec[close + 3..];
                return pattern.to_owned();
            }
        }
        return pathspec.to_owned();
    }

    match prefix {
        Some(p) if !p.is_empty() => PathBuf::from(p)
            .join(pathspec)
            .to_string_lossy()
            .to_string(),
        _ => pathspec.to_owned(),
    }
}
