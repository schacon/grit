//! Pathspec matching utilities shared across commands.

/// Parsed pathspec magic flags plus the underlying pattern.
#[derive(Debug, Clone, Copy)]
pub struct MagicPathspec<'a> {
    /// Whether matching paths should be excluded.
    pub exclude: bool,
    /// Whether the pathspec is rooted at the repository top.
    pub top: bool,
    /// The pattern body after removing any supported magic prefix.
    pub pattern: &'a str,
}

/// Parse the subset of pathspec magic currently shared across commands.
#[must_use]
pub fn parse_magic_pathspec(spec: &str) -> MagicPathspec<'_> {
    if let Some(pattern) = spec.strip_prefix(":/") {
        return MagicPathspec {
            exclude: false,
            top: true,
            pattern,
        };
    }
    if let Some(pattern) = spec.strip_prefix(":!") {
        return MagicPathspec {
            exclude: true,
            top: false,
            pattern,
        };
    }
    if let Some(rest) = spec.strip_prefix(":(") {
        if let Some(close) = rest.find(')') {
            let (magic, suffix) = rest.split_at(close);
            let pattern = &suffix[1..];
            let mut exclude = false;
            let mut top = false;
            for word in magic.split(',') {
                match word {
                    "exclude" => exclude = true,
                    "top" => top = true,
                    _ => {}
                }
            }
            return MagicPathspec {
                exclude,
                top,
                pattern,
            };
        }
    }
    MagicPathspec {
        exclude: false,
        top: false,
        pattern: spec,
    }
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
    if has_glob_chars(spec) {
        path == spec || path.starts_with(&format!("{spec}/")) || glob_match(spec, path)
    } else if let Some(prefix) = spec.strip_suffix('/') {
        path == prefix || path.starts_with(&format!("{prefix}/"))
    } else {
        path == spec || path.starts_with(&format!("{spec}/"))
    }
}
