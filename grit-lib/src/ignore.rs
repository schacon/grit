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
use crate::index::{Index, MODE_GITLINK};
use crate::objects::ObjectKind;
use crate::repo::Repository;
use crate::wildmatch::{wildmatch, WM_PATHNAME};

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
    /// Patterns from `git ls-files -x` / `--exclude` (Git `EXC_CMDL`), evaluated first.
    cli_rules: Vec<IgnoreRule>,
    global_rules: Vec<IgnoreRule>,
    info_rules: Vec<IgnoreRule>,
    /// Patterns from `git ls-files -X` / `--exclude-from` (Git `EXC_FILE`, after global/info).
    exclude_from_rules: Vec<IgnoreRule>,
    gitignore_cache: HashMap<String, Vec<IgnoreRule>>,
    /// Warnings emitted while loading in-tree `.gitignore` (e.g. symlink paths).
    pub warnings: Vec<String>,
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

    /// Append patterns from `ls-files --exclude-from` / `-X` files (relative paths resolve from `cwd`).
    ///
    /// Matches Git's `EXC_FILE` lists loaded after `core.excludesfile` and `.git/info/exclude`.
    pub fn add_exclude_from_files(&mut self, paths: &[PathBuf], cwd: &Path) -> Result<()> {
        for path in paths {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                cwd.join(path)
            };
            let display = path.display().to_string();
            let mut more =
                load_rules_from_file(&resolved, display, String::new(), false, &mut self.warnings)?;
            self.exclude_from_rules.append(&mut more);
        }
        Ok(())
    }

    /// Append patterns from `ls-files --exclude` / `-x` (Git command-line exclude list).
    pub fn add_cli_excludes(&mut self, patterns: &[String]) {
        for pat in patterns {
            if let Some(rule) = parse_rule_line(pat, 1, "--exclude option", "") {
                self.cli_rules.push(rule);
            }
        }
    }

    /// Take any warnings accumulated while loading ignore files (caller prints to stderr).
    #[must_use]
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
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

        let per_dir_rules = self.rules_for_path(repo, index, repo_rel_path)?;
        // Approximate Git precedence with a single "last match wins" pass: command-line and
        // `ls-files -X` patterns sit next to the standard file group, then per-directory
        // `.gitignore` (highest priority), matching the historical behavior that passed t0008.
        let all_rules = self
            .cli_rules
            .iter()
            .chain(self.global_rules.iter())
            .chain(self.info_rules.iter())
            .chain(self.exclude_from_rules.iter())
            .chain(per_dir_rules.iter())
            .collect::<Vec<_>>();
        for rule in &all_rules {
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

        let matched = refine_match_for_check_ignore_verbose(
            repo_rel_path,
            is_dir,
            ignored,
            matched,
            &all_rules,
        );

        Ok((ignored, matched))
    }

    fn rules_for_path(
        &mut self,
        repo: &Repository,
        index: Option<&Index>,
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
                let rules = load_gitignore_for_dir(repo, index, dir, &mut self.warnings)?;
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

    let mut sink = Vec::new();
    load_rules_from_file(&resolved, raw_path, String::new(), false, &mut sink)
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
    let mut sink = Vec::new();
    load_rules_from_file(
        &path,
        ".git/info/exclude".to_owned(),
        String::new(),
        false,
        &mut sink,
    )
}

fn load_gitignore_for_dir(
    repo: &Repository,
    index: Option<&Index>,
    dir: &str,
    warnings: &mut Vec<String>,
) -> Result<Vec<IgnoreRule>> {
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
    let rel_key = if dir.is_empty() {
        ".gitignore".to_owned()
    } else {
        format!("{dir}/.gitignore")
    };

    if let Some(content) = read_optional_text(&path)? {
        return parse_gitignore_content(&content, &source_display, dir, warnings);
    }

    if let Some(ix) = index {
        if let Some(entry) = ix.entries.iter().find(|e| {
            e.stage() == 0
                && std::str::from_utf8(&e.path)
                    .map(|p| p == rel_key.as_str())
                    .unwrap_or(false)
        }) {
            if let Ok(obj) = repo.odb.read(&entry.oid) {
                if obj.kind == ObjectKind::Blob {
                    if let Ok(text) = std::str::from_utf8(&obj.data) {
                        return parse_gitignore_content(text, &source_display, dir, warnings);
                    }
                }
            }
        }
    }

    Ok(Vec::new())
}

fn parse_gitignore_content(
    content: &str,
    source_display: &str,
    base_dir: &str,
    _warnings: &mut Vec<String>,
) -> Result<Vec<IgnoreRule>> {
    let mut rules = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if let Some(rule) = parse_rule_line(line, idx + 1, source_display, base_dir) {
            rules.push(rule);
        }
    }
    Ok(rules)
}

fn load_rules_from_file(
    path: &Path,
    source_display: String,
    base_dir: String,
    deny_symlink_gitignore: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<IgnoreRule>> {
    if deny_symlink_gitignore && path.exists() {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.file_type().is_symlink() {
                warnings.push(format!(
                    "warning: unable to access '{source_display}': Too many levels of symbolic links"
                ));
                return Ok(Vec::new());
            }
        }
    }
    let Some(content) = read_optional_text(path)? else {
        return Ok(Vec::new());
    };
    parse_gitignore_content(&content, &source_display, &base_dir, warnings)
}

/// Trims only *unescaped* trailing spaces, matching Git's `trim_trailing_spaces` in `dir.c`.
///
/// A backslash escapes the following byte; a run of spaces ending the line is removed only
/// when it is not part of an escape sequence (see t0008 "trailing whitespace is ignored").
fn trim_trailing_spaces_git(buf: &mut String) {
    let mut bytes = std::mem::take(buf).into_bytes();
    let mut last_space_start: Option<usize> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' => {
                if last_space_start.is_none() {
                    last_space_start = Some(i);
                }
                i += 1;
            }
            b'\\' => {
                last_space_start = None;
                i += 1;
                if i < bytes.len() {
                    i += 1;
                } else {
                    *buf = String::from_utf8_lossy(&bytes).into_owned();
                    return;
                }
            }
            _ => {
                last_space_start = None;
                i += 1;
            }
        }
    }
    if let Some(start) = last_space_start {
        bytes.truncate(start);
    }
    *buf = String::from_utf8_lossy(&bytes).into_owned();
}

fn parse_rule_line(
    line: &str,
    line_number: usize,
    source_display: &str,
    base_dir: &str,
) -> Option<IgnoreRule> {
    let mut raw_line = line.trim_end_matches('\r').to_owned();
    trim_trailing_spaces_git(&mut raw_line);
    if raw_line.is_empty() || raw_line.starts_with('#') {
        return None;
    }

    let pattern_text = raw_line.clone();
    let mut raw = raw_line;

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
    let body = raw;
    Some(IgnoreRule {
        source_display: source_display.to_owned(),
        line_number,
        pattern_text,
        negative,
        directory_only,
        anchored,
        has_slash,
        body,
        base_dir: base_dir.to_owned(),
    })
}

fn base_dir_depth(base: &str) -> usize {
    if base.is_empty() {
        return 0;
    }
    base.split('/').count()
}

/// Git's `check-ignore -v` attributes coverage to a parent `…/` directory rule when a redundant
/// positive pattern exists in a nested `.gitignore` under an already-ignored directory.
fn refine_match_for_check_ignore_verbose(
    repo_rel_path: &str,
    is_dir: bool,
    ignored: bool,
    matched: Option<IgnoreMatch>,
    rules: &[&IgnoreRule],
) -> Option<IgnoreMatch> {
    let Some(m) = matched else {
        return None;
    };
    if m.negative {
        return Some(m);
    }
    if !ignored {
        return Some(m);
    }
    let mut best: Option<&IgnoreRule> = None;
    for rule in rules {
        if !rule.directory_only {
            continue;
        }
        if !rule_matches(rule, repo_rel_path, is_dir) {
            continue;
        }
        match best {
            None => best = Some(rule),
            Some(b) if base_dir_depth(&rule.base_dir) < base_dir_depth(&b.base_dir) => {
                best = Some(rule);
            }
            Some(b)
                if base_dir_depth(&rule.base_dir) == base_dir_depth(&b.base_dir)
                    && rule.line_number < b.line_number =>
            {
                best = Some(rule);
            }
            _ => {}
        }
    }
    Some(
        best.map(|r| IgnoreMatch {
            source_display: r.source_display.clone(),
            line_number: r.line_number,
            pattern_text: r.pattern_text.clone(),
            negative: r.negative,
        })
        .unwrap_or(m),
    )
}

fn rule_matches(rule: &IgnoreRule, repo_rel_path: &str, is_dir: bool) -> bool {
    // Negated directory-only patterns containing `**` (e.g. `!data/**/`) only apply to directory
    // paths, not to files inside those directories. Matching them against ancestor paths for
    // files would incorrectly negate `data/**` for every file (see t0008 "directories and ** matches").
    if rule.directory_only && rule.negative && rule.body.contains("**") && !is_dir {
        return false;
    }

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
    wildmatch(pattern.as_bytes(), text.as_bytes(), WM_PATHNAME)
}

/// One line from a sparse-checkout specification blob (`--filter=sparse:oid=…`).
#[derive(Debug, Clone)]
struct SparsePattern {
    negative: bool,
    directory_only: bool,
    anchored: bool,
    has_slash: bool,
    body: String,
}

impl SparsePattern {
    fn from_line(line: &str) -> Option<Self> {
        let mut raw_line = line.trim_end_matches('\r').to_owned();
        trim_trailing_spaces_git(&mut raw_line);
        if raw_line.is_empty() || raw_line.starts_with('#') {
            return None;
        }

        let mut raw = raw_line;

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
        Some(Self {
            negative,
            directory_only,
            anchored,
            has_slash,
            body: raw,
        })
    }
}

/// Whether `p` matches `pathname` for sparse-checkout evaluation.
///
/// `as_directory` mirrors Git's `dtype == DT_DIR` pass when walking parent paths:
/// patterns with a trailing `/` in the sparse file (`PATTERN_FLAG_MUSTBEDIR`) only
/// participate in those iterations.
fn sparse_pattern_matches(p: &SparsePattern, pathname: &str, as_directory: bool) -> bool {
    if p.directory_only && !as_directory {
        return false;
    }
    if !p.has_slash && !p.anchored {
        sparse_unanchored_basename_matches(&p.body, pathname)
            || wildmatch(p.body.as_bytes(), pathname.as_bytes(), WM_PATHNAME)
    } else {
        wildmatch(p.body.as_bytes(), pathname.as_bytes(), WM_PATHNAME)
    }
}

fn sparse_pattern_matches_path(p: &SparsePattern, pathname: &str) -> bool {
    sparse_pattern_matches(p, pathname, false)
}

/// Parse sparse-checkout lines from a blob (same syntax as `info/sparse-checkout`).
#[must_use]
pub fn parse_sparse_patterns_from_blob(content: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    for line in content.lines() {
        let t = line.trim_end_matches('\r');
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        patterns.push(t.to_owned());
    }
    patterns
}

fn sparse_list_last_match(
    pathname: &str,
    as_directory: bool,
    parsed: &[SparsePattern],
) -> Option<bool> {
    for p in parsed.iter().rev() {
        if sparse_pattern_matches(p, pathname, as_directory) {
            return Some(!p.negative);
        }
    }
    None
}

/// Non-cone sparse-checkout inclusion, matching Git's `path_in_sparse_checkout`.
///
/// Walks from the full path toward parents (as in `dir.c:path_in_sparse_checkout_1`):
/// each step uses last-match-wins over patterns; `UNDECIDED` falls back to the parent
/// directory until the decision is made or the path is rejected at the top level.
#[must_use]
pub fn path_in_sparse_checkout(path: &str, lines: &[String]) -> bool {
    if path.is_empty() {
        return true;
    }
    let parsed: Vec<SparsePattern> = lines
        .iter()
        .filter_map(|l| SparsePattern::from_line(l))
        .collect();
    if parsed.is_empty() {
        return true;
    }

    let mut end = path.len();
    let mut as_directory = false;

    loop {
        let pathname = &path[..end];

        match sparse_list_last_match(pathname, as_directory, &parsed) {
            Some(true) => return true,
            Some(false) => return false,
            None => {
                if end == 0 {
                    return false;
                }
                if let Some(slash) = path[..end].rfind('/') {
                    end = slash;
                } else {
                    end = 0;
                }
                as_directory = true;
            }
        }
    }
}

/// Last-match-wins sparse semantics for `rev-list --filter=sparse:oid=…` (non-cone).
///
/// Unanchored patterns without `/` use Git-style basename rules: `pat` matches `pat`,
/// `pat.ext`, and `pat/…` paths. Anchored patterns and patterns containing `/` use
/// pathname wildmatch with `WM_PATHNAME`.
///
/// Returns `None` if no pattern matched (`UNDECIDED`).
#[must_use]
pub fn path_matches_sparse_pattern_list(pathname: &str, lines: &[String]) -> Option<bool> {
    let parsed: Vec<SparsePattern> = lines
        .iter()
        .filter_map(|l| SparsePattern::from_line(l))
        .collect();
    if parsed.is_empty() {
        return None;
    }
    for p in parsed.iter().rev() {
        if sparse_pattern_matches_path(p, pathname) {
            return Some(!p.negative);
        }
    }
    None
}

fn sparse_unanchored_basename_matches(pat: &str, path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    if basename == pat {
        return true;
    }
    if let Some(rest) = basename.strip_prefix(pat) {
        return rest.starts_with('.') || rest.starts_with('/');
    }
    if path == pat {
        return true;
    }
    path.starts_with(&format!("{pat}/"))
}

/// Returns the submodule path if `repo_rel_path` names something inside a gitlink entry.
#[must_use]
pub fn submodule_containing_path(repo_rel_path: &str, index: &Index) -> Option<String> {
    let mut best: Option<&str> = None;
    for entry in &index.entries {
        if entry.stage() != 0 || entry.mode != MODE_GITLINK {
            continue;
        }
        let Ok(p) = std::str::from_utf8(&entry.path) else {
            continue;
        };
        if repo_rel_path.len() > p.len()
            && repo_rel_path.starts_with(p)
            && repo_rel_path.as_bytes().get(p.len()) == Some(&b'/')
            && best.is_none_or(|b| p.len() > b.len())
        {
            best = Some(p);
        }
    }
    best.map(std::string::ToString::to_string)
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
    if path.starts_with(':') {
        return Ok(path.to_owned());
    }
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
