//! Ignore and exclude matching for `check-ignore`.
//!
//! This module implements a focused subset of Git ignore behavior:
//! per-directory `.gitignore`, `.git/info/exclude`, and `core.excludesfile`
//! with "last matching pattern wins" precedence.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::config::{parse_path, ConfigSet};
use crate::error::{Error, Result};
use crate::index::Index;
use crate::repo::Repository;

/// Metadata for a matching rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreMatch {
    /// The source file shown in verbose output.
    pub source_display: String,
    /// Line number in the source file (1-based).
    pub line_number: usize,
    /// Pattern text as written (excluding comments/blank lines).
    pub pattern_text: String,
    /// Whether this is a negated pattern (`!pattern`).
    pub negative: bool,
}

#[derive(Debug, Clone)]
struct IgnoreRule {
    source_display: String,
    line_number: usize,
    pattern_text: String,
    negative: bool,
    directory_only: bool,
    anchored: bool,
    has_slash: bool,
    body: String,
    base_dir: String,
}

/// Engine used to evaluate ignore patterns against repository-relative paths.
#[derive(Debug, Default)]
pub struct IgnoreMatcher {
    global_rules: Vec<IgnoreRule>,
    info_rules: Vec<IgnoreRule>,
    gitignore_cache: HashMap<String, Vec<IgnoreRule>>,
}

impl IgnoreMatcher {
    /// Build a matcher from repository exclude sources.
    ///
    /// # Parameters
    ///
    /// - `repo` - open repository.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if configured pattern files cannot be read.
    pub fn from_repository(repo: &Repository) -> Result<Self> {
        Ok(Self {
            global_rules: load_global_excludes(repo)?,
            info_rules: load_info_excludes(repo)?,
            ..Self::default()
        })
    }

    /// Check whether a repository-relative path is ignored.
    ///
    /// # Parameters
    ///
    /// - `repo` - repository handle.
    /// - `index` - optional index; when present, tracked entries are not ignored.
    /// - `repo_rel_path` - normalized repository-relative path with `/` separators.
    /// - `is_dir` - whether the queried path is a directory.
    ///
    /// # Returns
    ///
    /// Tuple `(ignored, match_info)` where `match_info` is the last matching
    /// pattern (including negated matches).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when a relevant `.gitignore` cannot be read.
    pub fn check_path(
        &mut self,
        repo: &Repository,
        index: Option<&Index>,
        repo_rel_path: &str,
        is_dir: bool,
    ) -> Result<(bool, Option<IgnoreMatch>)> {
        if is_tracked(index, repo_rel_path) {
            return Ok((false, None));
        }

        let mut matched: Option<IgnoreMatch> = None;
        let mut ignored = false;

        let per_dir_rules = self.rules_for_path(repo, repo_rel_path)?;
        for rule in self
            .global_rules
            .iter()
            .chain(self.info_rules.iter())
            .chain(per_dir_rules.iter())
        {
            if rule_matches(rule, repo_rel_path, is_dir) {
                matched = Some(IgnoreMatch {
                    source_display: rule.source_display.clone(),
                    line_number: rule.line_number,
                    pattern_text: rule.pattern_text.clone(),
                    negative: rule.negative,
                });
                ignored = !rule.negative;
            }
        }

        Ok((ignored, matched))
    }

    fn rules_for_path(
        &mut self,
        repo: &Repository,
        repo_rel_path: &str,
    ) -> Result<Vec<IgnoreRule>> {
        let parent = parent_dir(repo_rel_path);
        let mut dirs = Vec::new();
        dirs.push(String::new());
        if !parent.is_empty() {
            let mut cur = String::new();
            for segment in parent.split('/') {
                if !cur.is_empty() {
                    cur.push('/');
                }
                cur.push_str(segment);
                dirs.push(cur.clone());
            }
        }

        for dir in &dirs {
            if !self.gitignore_cache.contains_key(dir) {
                let rules = load_gitignore_for_dir(repo, dir)?;
                self.gitignore_cache.insert(dir.clone(), rules);
            }
        }

        let mut all: Vec<IgnoreRule> = Vec::new();
        for dir in dirs {
            if let Some(rules) = self.gitignore_cache.get(&dir) {
                all.extend(rules.iter().cloned());
            }
        }
        Ok(all)
    }
}

fn load_global_excludes(repo: &Repository) -> Result<Vec<IgnoreRule>> {
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let Some(raw_path) = config
        .get("core.excludesfile")
        .or_else(default_global_ignore_path)
    else {
        return Ok(Vec::new());
    };

    let expanded = parse_path(&raw_path);
    let resolved = if Path::new(&expanded).is_absolute() {
        PathBuf::from(&expanded)
    } else if let Some(work_tree) = &repo.work_tree {
        work_tree.join(&expanded)
    } else {
        repo.git_dir.join(&expanded)
    };

    load_rules_from_file(&resolved, raw_path, String::new())
}

fn default_global_ignore_path() -> Option<String> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(format!("{xdg}/git/ignore"));
        }
    }

    std::env::var("HOME")
        .ok()
        .map(|home| format!("{home}/.config/git/ignore"))
}

fn load_info_excludes(repo: &Repository) -> Result<Vec<IgnoreRule>> {
    let path = repo.git_dir.join("info/exclude");
    load_rules_from_file(&path, ".git/info/exclude".to_owned(), String::new())
}

fn load_gitignore_for_dir(repo: &Repository, dir: &str) -> Result<Vec<IgnoreRule>> {
    let Some(work_tree) = &repo.work_tree else {
        return Ok(Vec::new());
    };
    let path = if dir.is_empty() {
        work_tree.join(".gitignore")
    } else {
        work_tree.join(dir).join(".gitignore")
    };
    let source_display = if dir.is_empty() {
        ".gitignore".to_owned()
    } else {
        format!("{dir}/.gitignore")
    };
    load_rules_from_file(&path, source_display, dir.to_owned())
}

fn load_rules_from_file(
    path: &Path,
    source_display: String,
    base_dir: String,
) -> Result<Vec<IgnoreRule>> {
    let Some(content) = read_optional_text(path)? else {
        return Ok(Vec::new());
    };

    let mut rules = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if let Some(rule) = parse_rule_line(line, idx + 1, &source_display, &base_dir) {
            rules.push(rule);
        }
    }
    Ok(rules)
}

fn parse_rule_line(
    line: &str,
    line_number: usize,
    source_display: &str,
    base_dir: &str,
) -> Option<IgnoreRule> {
    let mut raw = line.trim_end().to_owned();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('#') {
        return None;
    }

    let mut negative = false;
    if let Some(rest) = raw.strip_prefix('!') {
        negative = true;
        raw = rest.to_owned();
    }
    if raw.is_empty() {
        return None;
    }

    let mut anchored = false;
    if let Some(rest) = raw.strip_prefix('/') {
        anchored = true;
        raw = rest.to_owned();
    }
    if raw.is_empty() {
        return None;
    }

    let mut directory_only = false;
    if let Some(rest) = raw.strip_suffix('/') {
        directory_only = true;
        raw = rest.to_owned();
    }
    if raw.is_empty() {
        return None;
    }

    let has_slash = raw.contains('/');
    Some(IgnoreRule {
        source_display: source_display.to_owned(),
        line_number,
        pattern_text: line.trim_end().to_owned(),
        negative,
        directory_only,
        anchored,
        has_slash,
        body: raw,
        base_dir: base_dir.to_owned(),
    })
}

fn rule_matches(rule: &IgnoreRule, repo_rel_path: &str, is_dir: bool) -> bool {
    let Some(rel_to_base) = strip_base(&rule.base_dir, repo_rel_path) else {
        return false;
    };

    if rule.directory_only {
        if rule.has_slash || rule.anchored {
            for ancestor in ancestor_dirs(rel_to_base, is_dir) {
                if glob_matches(&rule.body, &ancestor) {
                    return true;
                }
            }
            return false;
        }
        for ancestor in ancestor_dir_basenames(rel_to_base, is_dir) {
            if glob_matches(&rule.body, ancestor) {
                return true;
            }
        }
        return false;
    }

    if rule.has_slash || rule.anchored {
        return glob_matches(&rule.body, rel_to_base);
    }

    path_component_names(rel_to_base)
        .iter()
        .any(|name| glob_matches(&rule.body, name))
}

fn is_tracked(index: Option<&Index>, repo_rel_path: &str) -> bool {
    let Some(index) = index else {
        return false;
    };
    index.entries.iter().any(|entry| {
        entry.stage() == 0
            && std::str::from_utf8(&entry.path)
                .map(|path| path == repo_rel_path)
                .unwrap_or(false)
    })
}

fn read_optional_text(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::Io(err)),
    }
}

fn strip_base<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(path);
    }
    if path == base {
        return Some("");
    }
    let prefix = format!("{base}/");
    path.strip_prefix(&prefix)
}

fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => "",
    }
}

fn path_component_names(path: &str) -> Vec<&str> {
    if path.is_empty() {
        return Vec::new();
    }
    path.split('/').collect()
}

fn ancestor_dirs(path: &str, is_dir: bool) -> Vec<String> {
    let mut out = Vec::new();
    if path.is_empty() {
        return out;
    }
    let parts: Vec<&str> = path.split('/').collect();
    let max = if is_dir {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };
    for idx in 1..=max {
        out.push(parts[..idx].join("/"));
    }
    out
}

fn ancestor_dir_basenames(path: &str, is_dir: bool) -> Vec<&str> {
    let mut out = Vec::new();
    let parts: Vec<&str> = if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').collect()
    };
    let max = if is_dir {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };
    for item in parts.iter().take(max) {
        out.push(*item);
    }
    out
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    wildcard_match(pattern.as_bytes(), text.as_bytes())
}

fn wildcard_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut p = 0usize;
    let mut t = 0usize;
    let mut star_p = None;
    let mut star_t = 0usize;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
            continue;
        }
        if p < pattern.len() && pattern[p] == b'*' {
            star_p = Some(p);
            p += 1;
            star_t = t;
            continue;
        }
        if let Some(saved_p) = star_p {
            p = saved_p + 1;
            star_t += 1;
            t = star_t;
            continue;
        }
        return false;
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }

    p == pattern.len()
}

/// Convert a user-supplied path into a normalized repository-relative path.
///
/// # Parameters
///
/// - `repo` - repository handle.
/// - `cwd` - current working directory.
/// - `path` - user input path string.
///
/// # Errors
///
/// Returns [`Error::PathError`] if the path resolves outside the work tree.
pub fn normalize_repo_relative(repo: &Repository, cwd: &Path, path: &str) -> Result<String> {
    let Some(work_tree) = &repo.work_tree else {
        return Err(Error::PathError(
            "this operation must be run in a work tree".to_owned(),
        ));
    };
    let input = Path::new(path);
    let combined = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };
    let normalized = normalize_path(&combined);
    let rel = normalized
        .strip_prefix(work_tree)
        .map_err(|_| Error::PathError(format!("path '{path}' is outside repository work tree")))?;
    Ok(path_to_slash(rel))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_to_slash(path: &Path) -> String {
    let mut out = String::new();
    for (idx, component) in path.components().enumerate() {
        if idx > 0 {
            out.push('/');
        }
        out.push_str(&component.as_os_str().to_string_lossy());
    }
    out
}
