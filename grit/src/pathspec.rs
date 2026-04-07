//! Pathspec matching utilities shared across commands.

use anyhow::{bail, Context, Result};

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

/// Check if a string contains glob meta-characters.
pub fn has_glob_chars(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
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

    if magic.icase {
        let pattern_folded = pattern.to_ascii_lowercase();
        let path_folded = path.to_ascii_lowercase();
        if has_glob_chars(pattern) {
            glob_match(&pattern_folded, &path_folded)
        } else if let Some(prefix) = pattern_folded.strip_suffix('/') {
            path_folded == prefix || path_folded.starts_with(&format!("{prefix}/"))
        } else {
            path_folded == pattern_folded || path_folded.starts_with(&format!("{pattern_folded}/"))
        }
    } else if has_glob_chars(pattern) {
        glob_match(pattern, path)
    } else if let Some(prefix) = pattern.strip_suffix('/') {
        path == prefix || path.starts_with(&format!("{prefix}/"))
    } else {
        path == pattern || path.starts_with(&format!("{pattern}/"))
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
