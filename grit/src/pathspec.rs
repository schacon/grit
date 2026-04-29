//! CLI pathspec resolution helpers.

use std::path::{Path, PathBuf};

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

/// For exclude (and other cwd-relative) pathspec magic from a subdirectory, Git resolves the
/// pattern against the current directory (`:!sub/` from `repo/sub` → exclude `sub/sub/`).
fn prepend_cwd_to_short_exclude_pathspec(spec: &str, cwd: &str) -> Option<String> {
    let cwd = cwd.trim_end_matches('/');
    if cwd.is_empty() {
        return None;
    }
    let bytes = spec.as_bytes();
    if bytes.first().copied() != Some(b':') {
        return None;
    }
    // `:/path` is `:(top)` short form — exclude is relative to repo root, not cwd (t6132).
    if bytes.get(1).copied() == Some(b'/') {
        return None;
    }
    let mut i = 1usize;
    while i < bytes.len() && bytes[i] != b':' {
        let ch = bytes[i];
        if ch == b'^' {
            i += 1;
            continue;
        }
        let is_magic = matches!(ch, b'!' | b'/');
        if is_magic {
            i += 1;
            continue;
        }
        break;
    }
    if i < bytes.len() && bytes[i] == b':' {
        i += 1;
    }
    let pattern = spec.get(i..)?;
    if pattern.is_empty() || pattern.starts_with('/') {
        return None;
    }
    Some(format!("{}{}/{pattern}", &spec[..i], cwd))
}

/// Resolve a pathspec string to a path relative to the repository work tree.
///
/// `prefix` is the current directory relative to the work tree (no trailing slash),
/// or `None` when cwd is the work tree root.
#[must_use]
pub fn resolve_pathspec(pathspec: &str, work_tree: &Path, prefix: Option<&str>) -> String {
    // Git: `.` at repo root means "match the whole tree" (not an empty pathspec).
    // An empty resolved pathspec would match nothing and breaks `grep -- . t` max-depth.
    if pathspec == "." {
        return match prefix {
            Some(p) if !p.is_empty() => p.to_owned(),
            _ => ".".to_owned(),
        };
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
        if let Some(p) = prefix {
            if !p.is_empty() && !grit_lib::pathspec::literal_pathspecs_enabled() {
                let cwd_ps = format!("{}/", p.trim_end_matches('/'));
                if pathspec.starts_with(":(") {
                    if let Some(resolved) = resolve_magic_pathspec(pathspec, &cwd_ps) {
                        return resolved;
                    }
                    return pathspec.to_owned();
                }
                if grit_lib::pathspec::pathspec_is_exclude(pathspec) {
                    if let Some(fixed) = prepend_cwd_to_short_exclude_pathspec(pathspec, p) {
                        return fixed;
                    }
                }
            }
        }
        if let Some(rest) = pathspec.strip_prefix(":/") {
            // `:/!foo` / `:/^bar` — `:/` is `:(top)`; the tail is still short magic, not a literal path.
            if rest.starts_with('!') || rest.starts_with('^') {
                return pathspec.to_owned();
            }
            return rest.to_owned();
        }
        // Long magic `:(...)` must stay intact — `:(exclude)path` is not the same as `path`
        // (t6132-pathspec-exclude, grep --untracked with exclude pathspecs).
        if pathspec.starts_with(":(") {
            return pathspec.to_owned();
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
