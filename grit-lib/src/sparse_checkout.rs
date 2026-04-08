//! Sparse-checkout pattern parsing and path membership (cone and non-cone).
//!
//! Cone-mode parsing and matching follow Git's `add_pattern_to_hashsets` and
//! `path_matches_pattern_list` closely enough for `read-tree` and plumbing tests.

use std::collections::BTreeSet;

/// Parsed non-cone sparse-checkout patterns in file order (last match wins).
#[derive(Debug, Clone)]
pub struct NonConePatterns {
    lines: Vec<String>,
}

impl NonConePatterns {
    /// Build from already-trimmed pattern lines (non-cone mode).
    #[must_use]
    pub fn from_lines(lines: Vec<String>) -> Self {
        Self { lines }
    }

    /// Parse a sparse-checkout file into ordered patterns (non-cone mode).
    #[must_use]
    pub fn parse(content: &str) -> Self {
        let lines = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        Self { lines }
    }

    /// Returns true if `path` is included after applying ordered negated patterns.
    #[must_use]
    pub fn path_included(&self, path: &str) -> bool {
        let mut included = false;
        for raw in &self.lines {
            let (negated, core) = match raw.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, raw.as_str()),
            };
            let core = core.trim();
            if core.is_empty() || core.starts_with('#') {
                continue;
            }
            if non_cone_line_matches(core, path) {
                included = !negated;
            }
        }
        included
    }
}

fn glob_special_unescaped(name: &[u8]) -> bool {
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == b'\\' {
            i += 2;
            continue;
        }
        if matches!(name[i], b'*' | b'?' | b'[') {
            return true;
        }
        i += 1;
    }
    false
}

fn sparse_glob_match_star_crosses_slash(pattern: &[u8], text: &[u8]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (usize::MAX, 0usize);
    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_p = pi;
            star_t = ti;
            pi += 1;
        } else if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Same semantics as Git's plumbing for sparse-checkout file lines (`*` matches across `/`).
fn sparse_pattern_matches_git_non_cone(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim();
    if pat.is_empty() {
        return false;
    }

    let anchored = pat.starts_with('/');
    let pat = pat.trim_start_matches('/');

    if let Some(dir) = pat.strip_suffix('/') {
        if anchored && dir == "*" {
            return path.contains('/');
        }
        if anchored {
            return path == dir || path.starts_with(&format!("{dir}/"));
        }
        return path == dir
            || path.starts_with(&format!("{dir}/"))
            || path.split('/').any(|component| component == dir);
    }

    if anchored {
        return sparse_glob_match_star_crosses_slash(pat.as_bytes(), path.as_bytes());
    }
    sparse_glob_match_star_crosses_slash(pat.as_bytes(), path.as_bytes())
        || path.rsplit('/').next().is_some_and(|base| {
            sparse_glob_match_star_crosses_slash(pat.as_bytes(), base.as_bytes())
        })
}

fn non_cone_line_matches(pattern: &str, path: &str) -> bool {
    sparse_pattern_matches_git_non_cone(pattern, path)
}

/// Cone-mode sparse state: keys use a leading `/` (Git's internal form).
#[derive(Debug, Clone, Default)]
pub struct ConePatterns {
    pub full_cone: bool,
    pub recursive_slash: BTreeSet<String>,
    pub parent_slash: BTreeSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConeMatch {
    Undecided,
    Matched,
    MatchedRecursive,
    NotMatched,
}

impl ConePatterns {
    /// Parse sparse-checkout lines in cone mode. On structural failure returns `None` and
    /// callers should fall back to non-cone matching (and may print `warnings`).
    #[must_use]
    pub fn try_parse_with_warnings(content: &str, warnings: &mut Vec<String>) -> Option<Self> {
        let lines: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        let mut full_cone = false;
        let mut recursive: BTreeSet<String> = BTreeSet::new();
        let mut parents: BTreeSet<String> = BTreeSet::new();

        for line in lines {
            let (negated, rest) = if let Some(r) = line.strip_prefix('!') {
                (true, r)
            } else {
                (false, line)
            };

            if negated && rest == "/*/" {
                full_cone = false;
                continue;
            }
            if !negated && rest == "/*" {
                full_cone = true;
                continue;
            }

            if negated && rest.ends_with("/*/") && rest.starts_with('/') && rest.len() > 4 {
                let inner = &rest[1..rest.len() - 3];
                if inner.is_empty()
                    || inner.contains('/')
                    || glob_special_unescaped(inner.as_bytes())
                {
                    warnings.push(format!("warning: unrecognized negative pattern: '{rest}'"));
                    warnings.push("warning: disabling cone pattern matching".to_string());
                    return None;
                }
                let key = format!("/{inner}");
                if !recursive.contains(&key) {
                    warnings.push(format!("warning: unrecognized negative pattern: '{rest}'"));
                    warnings.push("warning: disabling cone pattern matching".to_string());
                    return None;
                }
                recursive.remove(&key);
                parents.insert(key);
                continue;
            }

            if negated {
                warnings.push(format!("warning: unrecognized negative pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }

            if rest == "/*" {
                continue;
            }

            if !rest.starts_with('/') {
                warnings.push(format!("warning: unrecognized pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }
            if rest.contains("**") {
                warnings.push(format!("warning: unrecognized pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }
            if rest.len() < 2 {
                warnings.push(format!("warning: unrecognized pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }

            let must_be_dir = rest.ends_with('/');
            let body = rest[1..].trim_end_matches('/');
            if body.is_empty() {
                warnings.push(format!("warning: unrecognized pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }
            if !must_be_dir {
                warnings.push(format!("warning: unrecognized pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }
            if glob_special_unescaped(body.as_bytes()) {
                warnings.push(format!("warning: unrecognized pattern: '{rest}'"));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }

            let key = format!("/{body}");
            if parents.contains(&key) {
                warnings.push(format!(
                    "warning: your sparse-checkout file may have issues: pattern '{rest}' is repeated"
                ));
                warnings.push("warning: disabling cone pattern matching".to_string());
                return None;
            }
            recursive.insert(key.clone());
            let parts: Vec<&str> = body.split('/').collect();
            for i in 1..parts.len() {
                let prefix = parts[..i].join("/");
                parents.insert(format!("/{prefix}"));
            }
        }

        Some(ConePatterns {
            full_cone,
            recursive_slash: recursive,
            parent_slash: parents,
        })
    }

    #[must_use]
    pub fn try_parse(content: &str) -> Option<Self> {
        let mut w = Vec::new();
        Self::try_parse_with_warnings(content, &mut w)
    }

    fn recursive_contains_parent(path: &str, recursive: &BTreeSet<String>) -> bool {
        let mut buf = String::from("/");
        buf.push_str(path);
        let mut slash_pos = buf.rfind('/');
        while let Some(pos) = slash_pos {
            if pos == 0 {
                break;
            }
            buf.truncate(pos);
            if recursive.contains(&buf) {
                return true;
            }
            slash_pos = buf.rfind('/');
        }
        false
    }

    /// Git `path_matches_pattern_list` for cone mode (`pathname` has no leading slash).
    fn path_matches_pattern_list(&self, pathname: &str) -> ConeMatch {
        if self.full_cone {
            return ConeMatch::Matched;
        }

        let mut parent_pathname = String::with_capacity(pathname.len() + 2);
        parent_pathname.push('/');
        parent_pathname.push_str(pathname);

        let slash_pos = if parent_pathname.ends_with('/') {
            let sp = parent_pathname.len() - 1;
            parent_pathname.push('-');
            sp
        } else {
            parent_pathname.rfind('/').unwrap_or(0)
        };

        if self.recursive_slash.contains(&parent_pathname) {
            return ConeMatch::MatchedRecursive;
        }

        if slash_pos == 0 {
            return ConeMatch::Matched;
        }

        let parent_key = parent_pathname[..slash_pos].to_string();
        if self.parent_slash.contains(&parent_key) {
            return ConeMatch::Matched;
        }

        if Self::recursive_contains_parent(pathname, &self.recursive_slash) {
            return ConeMatch::MatchedRecursive;
        }

        ConeMatch::NotMatched
    }

    /// Whether `path` (repository-relative, no leading slash) is inside the cone.
    #[must_use]
    pub fn path_included(&self, path: &str) -> bool {
        if path.is_empty() {
            return true;
        }

        let bytes = path.as_bytes();
        let mut end = bytes.len();
        let mut match_result = ConeMatch::Undecided;

        while end > 0 && match_result == ConeMatch::Undecided {
            let slice = path.get(..end).unwrap_or("");
            match_result = self.path_matches_pattern_list(slice);

            let mut slash = end.saturating_sub(1);
            while slash > 0 && bytes[slash] != b'/' {
                slash -= 1;
            }
            end = if bytes.get(slash) == Some(&b'/') {
                slash
            } else {
                0
            };
        }

        matches!(
            match_result,
            ConeMatch::Matched | ConeMatch::MatchedRecursive
        )
    }
}

/// Load sparse-checkout file; returns `(cone_parse_ok, cone, non_cone)`.
#[must_use]
pub fn load_sparse_checkout(
    git_dir: &std::path::Path,
    cone_config: bool,
) -> (bool, Option<ConePatterns>, NonConePatterns) {
    let mut w = Vec::new();
    load_sparse_checkout_with_warnings(git_dir, cone_config, &mut w)
}

/// Like [`load_sparse_checkout`] but appends cone-parse warnings (for stderr).
pub fn load_sparse_checkout_with_warnings(
    git_dir: &std::path::Path,
    cone_config: bool,
    warnings: &mut Vec<String>,
) -> (bool, Option<ConePatterns>, NonConePatterns) {
    let path = git_dir.join("info").join("sparse-checkout");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return (false, None, NonConePatterns { lines: Vec::new() });
    };
    let non_cone = NonConePatterns::parse(&content);
    if !cone_config {
        return (false, None, non_cone);
    }
    match ConePatterns::try_parse_with_warnings(&content, warnings) {
        Some(cone) => (true, Some(cone), non_cone),
        None => (false, None, non_cone),
    }
}

/// If `path` is included in the sparse checkout.
#[must_use]
pub fn path_in_sparse_checkout(
    path: &str,
    cone_config: bool,
    cone: Option<&ConePatterns>,
    non_cone: &NonConePatterns,
) -> bool {
    if cone_config {
        if let Some(c) = cone {
            return c.path_included(path);
        }
    }
    non_cone.path_included(path)
}

/// Mutable cone sparse state (Git `pattern_list` hashmaps) for building `sparse-checkout` files.
#[derive(Debug, Clone, Default)]
pub struct ConeWorkspace {
    pub recursive_slash: BTreeSet<String>,
    pub parent_slash: BTreeSet<String>,
}

impl ConeWorkspace {
    /// Build from parsed cone file content.
    #[must_use]
    pub fn from_cone_patterns(cp: &ConePatterns) -> Self {
        Self {
            recursive_slash: cp.recursive_slash.clone(),
            parent_slash: cp.parent_slash.clone(),
        }
    }

    /// Rebuild from a set of repository-relative directory paths (after pruning descendants).
    #[must_use]
    pub fn from_directory_list(dirs: &[String]) -> Self {
        let mut pruned: Vec<String> = dirs
            .iter()
            .map(|s| s.trim_start_matches('/').trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        pruned.sort();
        let mut kept: Vec<String> = Vec::new();
        for d in pruned {
            if kept
                .iter()
                .any(|p| d.starts_with(p) && d.as_bytes().get(p.len()) == Some(&b'/'))
            {
                continue;
            }
            kept.retain(|k| !(k.starts_with(&d) && k.as_bytes().get(d.len()) == Some(&b'/')));
            kept.push(d);
        }
        let mut ws = ConeWorkspace::default();
        for d in kept {
            ws.insert_directory(&d);
        }
        ws
    }

    /// Insert a repository-relative directory path (no leading slash).
    pub fn insert_directory(&mut self, rel: &str) {
        let rel = rel.trim_start_matches('/');
        let rel = rel.trim_end_matches('/');
        if rel.is_empty() {
            return;
        }
        let key = format!("/{rel}");
        if self.parent_slash.contains(&key) {
            return;
        }
        self.recursive_slash.insert(key.clone());
        let parts: Vec<&str> = rel.split('/').collect();
        for i in 1..parts.len() {
            let prefix = parts[..i].join("/");
            self.parent_slash.insert(format!("/{prefix}"));
        }
    }

    fn recursive_contains_parent(path_slash: &str, recursive: &BTreeSet<String>) -> bool {
        let mut buf = String::from(path_slash);
        let mut slash_pos = buf.rfind('/');
        while let Some(pos) = slash_pos {
            if pos == 0 {
                break;
            }
            buf.truncate(pos);
            if recursive.contains(&buf) {
                return true;
            }
            slash_pos = buf.rfind('/');
        }
        false
    }

    /// Serialize to `.git/info/sparse-checkout` cone format (includes `/*` and `!/*/` header).
    #[must_use]
    pub fn to_sparse_checkout_file(&self) -> String {
        let mut parent_only: Vec<&String> = self
            .parent_slash
            .iter()
            .filter(|p| {
                !self.recursive_slash.contains(*p)
                    && !Self::recursive_contains_parent(p, &self.recursive_slash)
            })
            .collect();
        parent_only.sort();

        let mut out = String::new();
        out.push_str("/*\n!/*/\n");

        for p in parent_only {
            let esc = escape_cone_path_component(p);
            out.push_str(&esc);
            out.push_str("/\n!");
            out.push_str(&esc);
            out.push_str("/*/\n");
        }

        let mut rec_only: Vec<&String> = self
            .recursive_slash
            .iter()
            .filter(|p| !Self::recursive_contains_parent(p, &self.recursive_slash))
            .collect();
        rec_only.sort();

        for p in rec_only {
            let esc = escape_cone_path_component(p);
            out.push_str(&esc);
            out.push_str("/\n");
        }
        out
    }

    /// Directory names for `git sparse-checkout list` in cone mode (no leading slash).
    #[must_use]
    pub fn list_cone_directories(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .recursive_slash
            .iter()
            .map(|s| s.trim_start_matches('/').to_string())
            .collect();
        v.sort();
        v
    }
}

fn escape_cone_path_component(path_with_leading_slash: &str) -> String {
    let mut out = String::new();
    for ch in path_with_leading_slash.chars() {
        if matches!(ch, '*' | '?' | '[' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
