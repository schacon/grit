//! Git-compatible path normalization and helpers for `test-tool path-utils`.
//! Logic matches `git/path.c` (`normalize_path_copy`, `longest_ancestor_length`,
//! `relative_path`, `strip_path_suffix`) and `git/remote.c` (`relative_url`).

use std::path::{Path, PathBuf};

#[inline]
fn is_dir_sep(c: u8) -> bool {
    c == b'/'
}

/// Purely textual path normalization matching Git's `normalize_path_copy`.
/// Returns `Err(())` when `..` would escape above the root (Git returns -1).
pub fn normalize_path_copy(src: &str) -> Result<String, ()> {
    let is_abs = src.starts_with('/');
    let raw_ends_dir = {
        let stripped = src.trim_end_matches('/');
        stripped.ends_with("/.")
            || stripped.ends_with("/..")
            || src.ends_with('/')
            || src == "."
            || src == ".."
    };
    let trailing_slash = raw_ends_dir && !src.is_empty();
    let mut stack: Vec<String> = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0usize;
    if is_abs {
        i = 1;
    }
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] == b'/' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'/' {
            i += 1;
        }
        let part = &src[start..i];
        if part == "." {
            continue;
        }
        if part == ".." {
            if stack.pop().is_none() {
                return Err(());
            }
        } else {
            stack.push(part.to_string());
        }
    }

    let mut out = if is_abs {
        if stack.is_empty() {
            "/".to_string()
        } else {
            "/".to_string() + &stack.join("/")
        }
    } else if stack.is_empty() {
        String::new()
    } else {
        stack.join("/")
    };
    if trailing_slash && !out.is_empty() && !out.ends_with('/') {
        out.push('/');
    }
    Ok(out)
}

fn chomp_trailing_dir_sep(path: &[u8], mut len: usize) -> usize {
    while len > 0 && is_dir_sep(path[len - 1]) {
        len -= 1;
    }
    len
}

/// Git's `stripped_path_suffix_offset` / `strip_path_suffix`.
pub fn strip_path_suffix(path: &str, suffix: &str) -> Option<String> {
    let path = path.as_bytes();
    let suffix = suffix.as_bytes();
    let mut path_len = path.len();
    let mut suffix_len = suffix.len();

    while suffix_len > 0 {
        if path_len == 0 {
            return None;
        }
        if is_dir_sep(path[path_len - 1]) {
            if !is_dir_sep(suffix[suffix_len - 1]) {
                return None;
            }
            path_len = chomp_trailing_dir_sep(path, path_len);
            suffix_len = chomp_trailing_dir_sep(suffix, suffix_len);
        } else if path[path_len - 1] != suffix[suffix_len - 1] {
            return None;
        } else {
            path_len -= 1;
            suffix_len -= 1;
        }
    }

    if path_len > 0 && !is_dir_sep(path[path_len - 1]) {
        return None;
    }
    let off = chomp_trailing_dir_sep(path, path_len);
    Some(String::from_utf8_lossy(&path[..off]).into_owned())
}

/// Git's `longest_ancestor_length` — normalizes `path` and each colon-separated prefix.
pub fn longest_ancestor_length(path: &str, prefixes_colon_sep: &str) -> Result<i32, ()> {
    let path = normalize_path_copy(path)?;
    if path == "/" {
        return Ok(-1);
    }
    let mut max_len: i64 = -1;
    for ceil_raw in prefixes_colon_sep.split(':') {
        if ceil_raw.is_empty() {
            continue;
        }
        let ceil = normalize_path_copy(ceil_raw)?;
        let mut len = ceil.len();
        if len > 0 && ceil.as_bytes()[len - 1] == b'/' {
            len -= 1;
        }
        let p = path.as_bytes();
        let c = ceil.as_bytes();
        if len > p.len() || len > c.len() || p[..len] != c[..len] {
            continue;
        }
        // Match git/path.c: need a '/' after the ceiling and another path component (not exact path).
        if len == p.len() || p[len] != b'/' || p.get(len + 1).is_none() {
            continue;
        }
        if len as i64 > max_len {
            max_len = len as i64;
        }
    }
    Ok(max_len as i32)
}

fn have_same_root(path1: &str, path2: &str) -> bool {
    let abs1 = path1.starts_with('/');
    let abs2 = path2.starts_with('/');
    (abs1 && abs2) || (!abs1 && !abs2)
}

/// Git's `relative_path` from `path.c` (POSIX subset).
pub fn relative_path<'a>(in_path: &'a str, prefix: &'a str, sb: &'a mut String) -> Option<&'a str> {
    let in_len = in_path.len();
    let prefix_len = prefix.len();
    let mut in_off = 0usize;
    let mut prefix_off = 0usize;
    let mut i = 0usize;
    let mut j = 0usize;

    if in_len == 0 {
        return Some("./");
    }
    if prefix_len == 0 {
        return Some(in_path);
    }

    if !have_same_root(in_path, prefix) {
        return Some(in_path);
    }

    let in_b = in_path.as_bytes();
    let pre_b = prefix.as_bytes();

    while i < prefix_len && j < in_len && pre_b[i] == in_b[j] {
        if is_dir_sep(pre_b[i]) {
            while i < prefix_len && is_dir_sep(pre_b[i]) {
                i += 1;
            }
            while j < in_len && is_dir_sep(in_b[j]) {
                j += 1;
            }
            prefix_off = i;
            in_off = j;
        } else {
            i += 1;
            j += 1;
        }
    }

    if i >= prefix_len && prefix_off < prefix_len {
        if j >= in_len {
            in_off = in_len;
        } else if is_dir_sep(in_b[j]) {
            while j < in_len && is_dir_sep(in_b[j]) {
                j += 1;
            }
            in_off = j;
        } else {
            i = prefix_off;
        }
    } else if j >= in_len && in_off < in_len && is_dir_sep(pre_b[i]) {
        while i < prefix_len && is_dir_sep(pre_b[i]) {
            i += 1;
        }
        in_off = in_len;
    }

    let in_suffix = &in_path[in_off..];
    let in_suffix_len = in_suffix.len();

    if i >= prefix_len {
        if in_suffix_len == 0 {
            return Some("./");
        }
        return Some(in_suffix);
    }

    sb.clear();
    sb.reserve(in_suffix_len.saturating_add(prefix_len * 3));

    while i < prefix_len {
        if is_dir_sep(pre_b[i]) {
            sb.push_str("../");
            while i < prefix_len && is_dir_sep(pre_b[i]) {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    if prefix_len > 0 && !is_dir_sep(pre_b[prefix_len - 1]) {
        sb.push_str("../");
    }
    sb.push_str(in_suffix);

    Some(sb.as_str())
}

fn find_last_dir_sep(path: &str) -> Option<usize> {
    path.rfind('/')
}

fn chop_last_dir(remoteurl: &mut String, is_relative: bool) -> Result<bool, ()> {
    if let Some(pos) = find_last_dir_sep(remoteurl.as_str()) {
        remoteurl.truncate(pos);
        return Ok(false);
    }
    if let Some(pos) = remoteurl.rfind(':') {
        remoteurl.truncate(pos);
        return Ok(true);
    }
    if is_relative || remoteurl == "." {
        return Err(());
    }
    *remoteurl = ".".to_string();
    Ok(false)
}

fn url_is_local_not_ssh(url: &str) -> bool {
    let colon = url.find(':');
    let slash = url.find('/');
    match (colon, slash) {
        (None, _) => true,
        (Some(ci), Some(si)) if si < ci => true,
        _ => false,
    }
}

fn starts_with_dot_slash_native(s: &str) -> bool {
    s.starts_with("./")
}

fn starts_with_dot_dot_slash_native(s: &str) -> bool {
    s.starts_with("../")
}

fn ends_with_slash(url: &str) -> bool {
    url.ends_with('/')
}

/// Git's `relative_url` from `remote.c` (POSIX; no DOS drive handling).
pub fn relative_url(remote_url: &str, url: &str, up_path: Option<&str>) -> Result<String, ()> {
    if !url_is_local_not_ssh(url) || url.starts_with('/') {
        return Ok(url.to_string());
    }

    let mut remoteurl = remote_url.to_string();
    let len = remoteurl.len();
    if len == 0 {
        return Err(());
    }
    if remoteurl.ends_with('/') {
        remoteurl.truncate(len - 1);
    }

    let is_relative = if !url_is_local_not_ssh(&remoteurl) || remoteurl.starts_with('/') {
        false
    } else {
        if !starts_with_dot_slash_native(&remoteurl)
            && !starts_with_dot_dot_slash_native(&remoteurl)
        {
            remoteurl = format!("./{remoteurl}");
        }
        true
    };

    let mut url_rest = url;
    let mut colonsep = false;
    while !url_rest.is_empty() {
        if starts_with_dot_dot_slash_native(url_rest) {
            url_rest = &url_rest[3..];
            let seg = chop_last_dir(&mut remoteurl, is_relative)?;
            colonsep |= seg;
        } else if starts_with_dot_slash_native(url_rest) {
            url_rest = &url_rest[2..];
        } else {
            break;
        }
    }

    let sep = if colonsep { ":" } else { "/" };
    let mut combined = format!("{remoteurl}{sep}{url_rest}");
    if ends_with_slash(url) && combined.ends_with('/') {
        combined.pop();
    }

    let out = if starts_with_dot_slash_native(&combined) {
        combined[2..].to_string()
    } else {
        combined
    };

    match up_path {
        Some(up) if is_relative => Ok(format!("{up}{out}")),
        _ => Ok(out),
    }
}

pub fn is_absolute_path_unix(path: &str) -> bool {
    path.starts_with('/')
}

/// Like Git's `strbuf_realpath` / `test-tool path-utils real_path`: resolve symlinks by
/// walking path components (so symlink targets are interpreted at each step), then if the
/// leaf is missing, resolve the longest existing prefix and append the remainder.
pub fn real_path_resolving(path: &str) -> PathBuf {
    let abs = if path.starts_with('/') {
        path.to_string()
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let joined = format!("{}/{}", cwd.display(), path);
        normalize_path_copy(&joined).unwrap_or(joined)
    };
    let p = Path::new(&abs);
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    let mut cur = PathBuf::from("/");
    for part in abs.trim_start_matches('/').split('/') {
        if part.is_empty() {
            continue;
        }
        cur.push(part);
        if let Ok(c) = cur.canonicalize() {
            cur = c;
        } else if let Ok(target) = std::fs::read_link(&cur) {
            cur.pop();
            cur.push(target);
            if let Ok(c) = cur.canonicalize() {
                cur = c;
            }
        }
    }
    if cur.exists() {
        return cur;
    }
    let mut base = cur.clone();
    let mut missing = Vec::new();
    while !base.as_os_str().is_empty() && !base.exists() {
        missing.push(base.file_name().unwrap_or_default().to_owned());
        if !base.pop() {
            break;
        }
    }
    if base.as_os_str().is_empty() {
        base = PathBuf::from("/");
    }
    let Ok(mut resolved) = base.canonicalize() else {
        return cur;
    };
    while let Some(name) = missing.pop() {
        resolved.push(name);
    }
    resolved
}

/// Git's `abspath_part_inside_repo` from `setup.c` (POSIX).
///
/// Strips the work tree from an absolute, normalized path, preserving symlink path
/// components when they are still under the work tree as a string prefix.
pub fn abspath_part_inside_repo(path: &str, work_tree: &Path) -> Option<String> {
    let normalized = normalize_path_copy(path).ok()?;
    if !normalized.starts_with('/') {
        return None;
    }
    let wt_display = work_tree.to_string_lossy();
    let wt_trim: &str = if wt_display == "/" {
        "/"
    } else {
        wt_display.trim_end_matches('/')
    };
    let wt_len = wt_trim.len();
    let p = normalized.as_str();
    let len = p.len();

    if wt_len <= len && p.starts_with(wt_trim) {
        if len > wt_len && p.as_bytes()[wt_len] == b'/' {
            return Some(p[wt_len + 1..].to_string());
        }
        if len == wt_len {
            return Some(String::new());
        }
        if wt_len > 0 && wt_trim.as_bytes()[wt_len - 1] == b'/' {
            return Some(p[wt_len..].trim_start_matches('/').to_string());
        }
    }

    let wt_canon = std::fs::canonicalize(work_tree).ok()?;
    let mut cum = String::new();
    for seg in p.split('/').filter(|s| !s.is_empty()) {
        if cum.is_empty() {
            cum.push('/');
            cum.push_str(seg);
        } else {
            cum.push('/');
            cum.push_str(seg);
        }
        let rp = std::fs::canonicalize(Path::new(&cum)).ok()?;
        if rp == wt_canon {
            if p.len() == cum.len() {
                return Some(String::new());
            }
            if p.as_bytes().get(cum.len()) == Some(&b'/') {
                return Some(p[cum.len() + 1..].to_string());
            }
        }
    }
    let full = std::fs::canonicalize(Path::new(p)).ok()?;
    if full == wt_canon {
        return Some(String::new());
    }
    None
}

/// Git `setup.c` `prefix_path_gently` (POSIX).
pub fn prefix_path_gently(prefix: &str, path: &str, work_tree: &Path) -> Option<String> {
    if path.starts_with('/') {
        let n = normalize_path_copy(path).ok()?;
        abspath_part_inside_repo(&n, work_tree)
    } else {
        let concat = format!("{prefix}{path}");
        normalize_path_copy(&concat).ok()
    }
}
