//! Pathspec matching utilities shared across commands.

use anyhow::{bail, Context, Result};
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

/// Read pathspec entries from raw file bytes (stdin or file), matching Git's
/// `--pathspec-from-file` / `--pathspec-file-nul` rules.
///
/// * **NUL mode:** entries are separated by `NUL`; each segment must not use
///   C-style quoted lines (Git rejects quoted pathspecs in this mode).
/// * **Line mode:** entries are separated by `LF`; optional `CR` before `LF`
///   is stripped; optional trailing line without a final newline is included;
///   double-quoted lines are C-unquoted (including octal escapes).
pub fn parse_pathspecs_from_source(data: &[u8], nul_terminated: bool) -> Result<Vec<String>> {
    if nul_terminated {
        let mut out = Vec::new();
        for chunk in data.split(|b| *b == 0) {
            if chunk.is_empty() {
                continue;
            }
            let s = String::from_utf8_lossy(chunk);
            let t = s.trim();
            if t.starts_with('"') {
                bail!("pathspec-from-file: line is not NUL terminated: {}", t);
            }
            out.push(t.to_string());
        }
        return Ok(out);
    }

    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if line.starts_with('"') && line.ends_with('"') && line.len() >= 2 {
            out.push(unquote_c_style_pathspec_line(line)?);
        } else {
            out.push(line.to_string());
        }
    }
    Ok(out)
}

/// Unquote a single `--pathspec-from-file` line that is wrapped in double quotes.
fn unquote_c_style_pathspec_line(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') || bytes.len() < 2 {
        bail!("invalid C-style quoting: {s}");
    }

    let inner = &bytes[1..bytes.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' {
            out.push(inner[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= inner.len() {
            bail!("invalid escape at end of string");
        }
        match inner[i] {
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            c if c.is_ascii_digit() => {
                if i + 2 >= inner.len() {
                    bail!("truncated octal escape");
                }
                let oct = std::str::from_utf8(&inner[i..i + 3]).context("invalid octal bytes")?;
                out.push(u8::from_str_radix(oct, 8).context("invalid octal escape value")?);
                i += 2;
            }
            other => bail!("invalid escape sequence \\{}", char::from(other)),
        }
        i += 1;
    }
    String::from_utf8(out).context("invalid UTF-8 in quoted pathspec")
}

/// Check whether a path matches a pathspec (magic tokens and `GIT_*_PATHSPECS` globals).
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
pub(crate) struct PathspecMagic {
    pub(crate) icase: bool,
    pub(crate) prefix: Option<String>,
}

pub(crate) fn parse_magic(spec: &str) -> (PathspecMagic, &str) {
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
