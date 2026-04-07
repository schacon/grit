//! Gitattributes parsing and pattern matching for `check-attr` and validation.
//!
//! Implements Git-consistent rule ordering, macro expansion (`[attr]`), `binary`
//! expansion, `**` globbing via [`crate::wildmatch`], and optional case folding
//! for `core.ignorecase`.

use crate::config::parse_path;
use crate::config::ConfigSet;
use crate::index::normalize_mode;
use crate::index::Index;
use crate::index::MODE_EXECUTABLE;
use crate::index::MODE_GITLINK;
use crate::index::MODE_REGULAR;
use crate::index::MODE_SYMLINK;
use crate::index::MODE_TREE;
use crate::objects::parse_tree;
use crate::objects::ObjectId;
use crate::objects::ObjectKind;
use crate::odb::Odb;
use crate::repo::Repository;
use crate::rev_parse::resolve_revision;
use crate::wildmatch::{wildmatch, WM_CASEFOLD, WM_PATHNAME};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Maximum length of a single `.gitattributes` line (bytes), matching Git.
pub const MAX_ATTR_LINE_BYTES: usize = 2048;

/// Maximum `.gitattributes` file size (bytes) before Git ignores the file.
pub const MAX_ATTR_FILE_BYTES: usize = 100 * 1024 * 1024;

/// Parsed attribute value for display (`check-attr` output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrValue {
    Set,
    /// Explicit `-attr` in a rule — `check-attr` prints `unset`.
    Unset,
    /// Macro body `!attr` — clears the attribute to *unspecified* (not `unset`).
    Clear,
    Value(String),
}

impl AttrValue {
    /// Text form as printed by `git check-attr`.
    #[must_use]
    pub fn display(&self) -> &str {
        match self {
            AttrValue::Set => "set",
            AttrValue::Unset => "unset",
            AttrValue::Clear => "unspecified",
            AttrValue::Value(v) => v.as_str(),
        }
    }
}

/// One line in a gitattributes file.
#[derive(Debug, Clone)]
pub struct AttrRule {
    /// Normalized pattern (repo-relative, `/` separators).
    pub pattern: String,
    /// If true, this rule was discarded (negative pattern) after emitting a warning.
    pub skip: bool,
    /// 1-based line number in the source file.
    pub line: usize,
    /// Attribute assignments in source order (last wins for duplicates on this line).
    pub attrs: Vec<(String, AttrValue)>,
}

/// Macro definitions from `[attr]name ...` lines.
#[derive(Debug, Clone, Default)]
pub struct MacroTable {
    /// Maps macro name → list of assignments (e.g. `!test` → unset test).
    pub defs: HashMap<String, Vec<(String, AttrValue)>>,
}

/// Result of parsing a gitattributes file.
#[derive(Debug, Default)]
pub struct ParsedGitAttributes {
    pub rules: Vec<AttrRule>,
    pub macros: MacroTable,
    pub warnings: Vec<String>,
}

/// Returns true if `name` is reserved (`builtin_*` except the real builtin names Git allows).
#[must_use]
pub fn is_reserved_builtin_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("builtin_") else {
        return false;
    };
    matches!(rest, "objectmode")
}

/// Validate user-defined attribute names in parsed rules (for `git add`).
///
/// Returns an error string matching Git when a rule uses an invalid `builtin_*` name.
pub fn validate_rules_for_add(
    rules: &[AttrRule],
    display_path: &str,
) -> std::result::Result<(), String> {
    for rule in rules {
        if rule.skip {
            continue;
        }
        for (name, _) in &rule.attrs {
            if name.starts_with("builtin_") && !is_reserved_builtin_name(name) {
                return Err(format!(
                    "{name} is not a valid attribute name: {display_path}:{}",
                    rule.line
                ));
            }
        }
    }
    Ok(())
}

/// Collect warnings for invalid `builtin_*` assignments (check-attr continues).
pub fn builtin_warnings_for_rules(rules: &[AttrRule], display_path: &str) -> Vec<String> {
    let mut w = Vec::new();
    for rule in rules {
        if rule.skip {
            continue;
        }
        for (name, _) in &rule.attrs {
            if name == "builtin_objectmode" {
                w.push(format!(
                    "builtin_objectmode is not a valid attribute name: {display_path}:{}",
                    rule.line
                ));
            } else if name.starts_with("builtin_") && !is_reserved_builtin_name(name) {
                w.push(format!(
                    "{name} is not a valid attribute name: {display_path}:{}",
                    rule.line
                ));
            }
        }
    }
    w
}

fn default_global_attributes_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("git/attributes"));
        }
    }
    Some(PathBuf::from(home).join(".config/git/attributes"))
}

fn global_attributes_path(
    repo: &Repository,
) -> std::result::Result<Option<PathBuf>, crate::error::Error> {
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    if let Some(path) = config.get("core.attributesfile") {
        return Ok(Some(PathBuf::from(parse_path(&path))));
    }
    Ok(default_global_attributes_path())
}

/// Read a `.gitattributes` path; if it is a symlink, record an error and skip (in-tree rules).
fn read_gitattributes_maybe_symlink(
    path: &Path,
    display: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        warnings.push(format!(
            "unable to access '{display}': Too many levels of symbolic links"
        ));
        return None;
    }
    fs::read_to_string(path).ok()
}

/// Parse one gitattributes file from disk (follows symlinks only when reading global/info).
pub fn parse_gitattributes_file_content(content: &str, display_path: &str) -> ParsedGitAttributes {
    parse_gitattributes_content_impl(content, display_path, false)
}

fn parse_gitattributes_content_impl(
    content: &str,
    display_path: &str,
    from_blob: bool,
) -> ParsedGitAttributes {
    let mut out = ParsedGitAttributes::default();
    for (idx, raw_line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let line_bytes = raw_line.as_bytes();
        if line_bytes.len() > MAX_ATTR_LINE_BYTES {
            out.warnings.push(format!(
                "warning: ignoring overly long attributes line {line_no}"
            ));
            continue;
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        parse_one_line(line, line_no, display_path, from_blob, &mut out);
    }
    out.warnings
        .extend(builtin_warnings_for_rules(&out.rules, display_path));
    out
}

fn parse_one_line(
    line: &str,
    line_no: usize,
    display_path: &str,
    from_blob: bool,
    out: &mut ParsedGitAttributes,
) {
    let tokens = tokenize_attr_line(line);
    if tokens.is_empty() {
        return;
    }
    let mut it = tokens.into_iter();
    let first = it.next().unwrap_or_default();
    if let Some(rest) = first.strip_prefix("[attr]") {
        let macro_name = rest.to_string();
        let mut attrs = Vec::new();
        for t in it {
            push_attr_token(&t, &mut attrs, &out.macros, true);
        }
        out.macros.defs.insert(macro_name, attrs);
        return;
    }
    let pattern = first;
    if pattern.starts_with('!') && !pattern.starts_with("\\!") {
        out.warnings
            .push("Negative patterns are ignored".to_string());
        return;
    }
    let mut pattern = pattern.replace("\\!", "!");
    if pattern.len() > 1 && pattern.starts_with(' ') && !pattern[1..].contains(' ') {
        pattern = pattern.trim_start_matches(' ').to_string();
    }
    let mut attrs = Vec::new();
    for t in it {
        push_attr_token(&t, &mut attrs, &out.macros, false);
    }
    if attrs.is_empty() {
        return;
    }
    let skip = false;
    out.rules.push(AttrRule {
        pattern,
        skip,
        line: line_no,
        attrs,
    });
    let _ = line_no;
    let _ = display_path;
    let _ = from_blob;
}

fn push_attr_token(
    tok: &str,
    attrs: &mut Vec<(String, AttrValue)>,
    _macros: &MacroTable,
    in_macro_def: bool,
) {
    if tok == "binary" {
        attrs.push(("text".into(), AttrValue::Unset));
        attrs.push(("diff".into(), AttrValue::Unset));
        attrs.push(("merge".into(), AttrValue::Unset));
        attrs.push(("binary".into(), AttrValue::Set));
        return;
    }
    if in_macro_def {
        if let Some(rest) = tok.strip_prefix('!') {
            attrs.push((rest.to_string(), AttrValue::Clear));
            return;
        }
    }
    if let Some(rest) = tok.strip_prefix('-') {
        attrs.push((rest.to_string(), AttrValue::Unset));
        return;
    }
    if let Some((k, v)) = tok.split_once('=') {
        attrs.push((k.to_string(), AttrValue::Value(v.to_string())));
        return;
    }
    attrs.push((tok.to_string(), AttrValue::Set));
}

/// Tokenize a line into space-separated tokens with double-quote support.
fn tokenize_attr_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                continue;
            }
            if c == '"' {
                in_quotes = false;
                continue;
            }
            cur.push(c);
            continue;
        }
        if c.is_whitespace() {
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
            continue;
        }
        if c == '"' {
            in_quotes = true;
            continue;
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Match a single gitattributes pattern against a repo-relative path.
#[must_use]
pub fn attr_pattern_matches(pattern: &str, rel_path: &str, icase: bool) -> bool {
    let flags_base = if icase { WM_CASEFOLD } else { 0 };
    if !pattern.contains('/') {
        let basename = rel_path.rsplit('/').next().unwrap_or(rel_path);
        wildmatch(
            pattern.as_bytes(),
            basename.as_bytes(),
            flags_base | WM_PATHNAME,
        )
    } else {
        wildmatch(
            pattern.as_bytes(),
            rel_path.as_bytes(),
            flags_base | WM_PATHNAME,
        )
    }
}

/// Expand macros and `binary` for one rule's assignments into flat (name, value) pairs.
fn expand_rule_attrs(rule: &AttrRule, macros: &MacroTable) -> HashMap<String, AttrValue> {
    let mut flat: Vec<(String, AttrValue)> = Vec::new();
    for (name, val) in &rule.attrs {
        if name == "binary" {
            flat.push(("text".into(), AttrValue::Unset));
            flat.push(("diff".into(), AttrValue::Unset));
            flat.push(("merge".into(), AttrValue::Unset));
            flat.push(("binary".into(), AttrValue::Set));
            continue;
        }
        if let Some(exp) = macros.defs.get(name) {
            flat.push((name.clone(), val.clone()));
            for (n, v) in exp {
                flat.push((n.clone(), v.clone()));
            }
        } else {
            flat.push((name.clone(), val.clone()));
        }
    }
    let mut map: HashMap<String, AttrValue> = HashMap::new();
    for (n, v) in flat {
        match v {
            AttrValue::Clear => {
                map.remove(&n);
            }
            _ => {
                map.insert(n, v);
            }
        }
    }
    map
}

/// Merge assignments: later rules override earlier; within one expanded rule, last wins.
pub fn collect_attrs_for_path(
    rules: &[AttrRule],
    macros: &MacroTable,
    rel_path: &str,
    icase: bool,
) -> HashMap<String, AttrValue> {
    let mut map: HashMap<String, AttrValue> = HashMap::new();
    for rule in rules {
        if rule.skip {
            continue;
        }
        if !attr_pattern_matches(&rule.pattern, rel_path, icase) {
            continue;
        }
        let expanded = expand_rule_attrs(rule, macros);
        for (n, v) in expanded {
            match v {
                AttrValue::Clear => {
                    map.remove(&n);
                }
                _ => {
                    map.insert(n, v);
                }
            }
        }
    }
    map
}

/// Quote a path for `check-attr` output (C-style) when needed.
#[must_use]
pub fn quote_path_for_check_attr(path: &str) -> String {
    let needs = path
        .chars()
        .any(|c| c.is_control() || c == '"' || c == '\\');
    if !needs {
        return path.to_string();
    }
    let mut s = String::new();
    s.push('"');
    for c in path.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            _ if c.is_control() => s.push_str(&format!("\\{:o}", c as u32)),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Normalize `.` / `..` segments in a repo-relative path string.
#[must_use]
pub fn normalize_rel_path(path: &str) -> String {
    let p = Path::new(path);
    let mut stack: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => stack.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => {
                let _ = stack.pop();
            }
            Component::CurDir => {}
            _ => {}
        }
    }
    stack.join("/")
}

/// Resolve a user path to a repo-relative path (forward slashes).
pub fn path_relative_to_worktree(
    repo: &Repository,
    path_str: &str,
) -> std::result::Result<String, String> {
    let wt = repo
        .work_tree
        .as_ref()
        .ok_or_else(|| "bare repository — no work tree".to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let p = Path::new(path_str);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let abs = abs.canonicalize().map_err(|e| e.to_string())?;
    let wt = wt.canonicalize().map_err(|e| e.to_string())?;
    let rel = abs
        .strip_prefix(&wt)
        .map_err(|_| format!("path outside repository: {}", path_str))?;
    Ok(normalize_rel_path(
        rel.to_str().ok_or_else(|| "invalid path".to_string())?,
    ))
}

fn collect_nested_gitattributes_dirs(work_tree: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    walk_dirs(work_tree, work_tree, &mut dirs);
    dirs.sort_by(|a, b| {
        let da = a.components().count();
        let db = b.components().count();
        da.cmp(&db).then_with(|| a.cmp(b))
    });
    dirs
}

fn walk_dirs(root: &Path, cur: &Path, dirs: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(cur) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let ft = e.file_type().ok();
        if ft.is_some_and(|t| t.is_dir()) {
            if p.file_name() == Some(OsStr::new(".git")) {
                continue;
            }
            let rel = p.strip_prefix(root).unwrap_or(&p);
            dirs.push(rel.to_path_buf());
            walk_dirs(root, &p, dirs);
        }
    }
}

/// Load the full stack of attribute rules for a normal repository (working tree).
pub fn load_gitattributes_stack(
    repo: &Repository,
    work_tree: &Path,
) -> std::result::Result<ParsedGitAttributes, crate::error::Error> {
    let mut merged = ParsedGitAttributes::default();

    if let Some(g) = global_attributes_path(repo)? {
        if g.exists()
            && !g
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        {
            if let Ok(content) = fs::read_to_string(&g) {
                if content.len() <= MAX_ATTR_FILE_BYTES {
                    let mut p =
                        parse_gitattributes_file_content(&content, g.to_string_lossy().as_ref());
                    merged.rules.append(&mut p.rules);
                    merged.macros.defs.extend(p.macros.defs.drain());
                    merged.warnings.append(&mut p.warnings);
                } else {
                    merged.warnings.push(format!(
                        "warning: ignoring overly large gitattributes file '{}'",
                        g.display()
                    ));
                }
            }
        }
    }

    let root_ga = work_tree.join(".gitattributes");
    if let Some(content) =
        read_gitattributes_maybe_symlink(&root_ga, ".gitattributes", &mut merged.warnings)
    {
        if content.len() <= MAX_ATTR_FILE_BYTES {
            let mut p = parse_gitattributes_file_content(&content, ".gitattributes");
            merged.rules.append(&mut p.rules);
            merged.macros.defs.extend(p.macros.defs.drain());
            merged.warnings.append(&mut p.warnings);
        } else {
            merged.warnings.push(
                "warning: ignoring overly large gitattributes file '.gitattributes'".to_string(),
            );
        }
    }

    for rel in collect_nested_gitattributes_dirs(work_tree) {
        let ga = work_tree.join(&rel).join(".gitattributes");
        if let Some(content) = read_gitattributes_maybe_symlink(
            &ga,
            &format!("{}/.gitattributes", rel.display()),
            &mut merged.warnings,
        ) {
            if content.len() > MAX_ATTR_FILE_BYTES {
                merged.warnings.push(format!(
                    "warning: ignoring overly large gitattributes file '{}'",
                    ga.display()
                ));
                continue;
            }
            let prefix = rel.to_string_lossy().replace('\\', "/");
            let mut p = parse_gitattributes_file_content(&content, &ga.to_string_lossy());
            for mut r in p.rules.drain(..) {
                if !prefix.is_empty() {
                    r.pattern = format!("{prefix}/{}", r.pattern);
                }
                merged.rules.push(r);
            }
            merged.macros.defs.extend(p.macros.defs.drain());
            merged.warnings.append(&mut p.warnings);
        }
    }

    let info = repo.git_dir.join("info/attributes");
    if info.exists() {
        if let Ok(content) = fs::read_to_string(&info) {
            if content.len() <= MAX_ATTR_FILE_BYTES {
                let mut p = parse_gitattributes_file_content(&content, "info/attributes");
                merged.rules.append(&mut p.rules);
                merged.macros.defs.extend(p.macros.defs.drain());
                merged.warnings.append(&mut p.warnings);
            }
        }
    }

    Ok(merged)
}

/// Bare repository: only `info/attributes` from disk (no in-repo `.gitattributes` file).
pub fn load_gitattributes_bare(
    repo: &Repository,
) -> std::result::Result<ParsedGitAttributes, crate::error::Error> {
    let mut merged = ParsedGitAttributes::default();
    if let Some(g) = global_attributes_path(repo)? {
        if g.exists() {
            if let Ok(content) = fs::read_to_string(&g) {
                if content.len() <= MAX_ATTR_FILE_BYTES {
                    let mut p =
                        parse_gitattributes_file_content(&content, g.to_string_lossy().as_ref());
                    merged.rules.append(&mut p.rules);
                    merged.macros.defs.extend(p.macros.defs.drain());
                    merged.warnings.append(&mut p.warnings);
                }
            }
        }
    }
    let info = repo.git_dir.join("info/attributes");
    if info.exists() {
        if let Ok(content) = fs::read_to_string(&info) {
            if content.len() <= MAX_ATTR_FILE_BYTES {
                let mut p = parse_gitattributes_file_content(&content, "info/attributes");
                merged.rules.append(&mut p.rules);
                merged.macros.defs.extend(p.macros.defs.drain());
                merged.warnings.append(&mut p.warnings);
            }
        }
    }
    Ok(merged)
}

/// Read `.gitattributes` blob from a tree object at `tree_oid`, recursively.
pub fn load_gitattributes_from_tree(
    odb: &Odb,
    tree_oid: &ObjectId,
) -> std::result::Result<ParsedGitAttributes, crate::error::Error> {
    let mut merged = ParsedGitAttributes::default();
    walk_tree_attrs(odb, tree_oid, "", &mut merged)?;
    Ok(merged)
}

fn walk_tree_attrs(
    odb: &Odb,
    tree_oid: &ObjectId,
    prefix: &str,
    merged: &mut ParsedGitAttributes,
) -> std::result::Result<(), crate::error::Error> {
    let obj = odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    let entries = parse_tree(&obj.data)?;
    for e in entries {
        let name = String::from_utf8_lossy(&e.name).to_string();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        match e.mode {
            0o040000 => {
                walk_tree_attrs(odb, &e.oid, &path, merged)?;
            }
            0o100644 | 0o100755 | 0o120000 => {
                if name == ".gitattributes" {
                    let oid = e.oid;
                    {
                        let blob = odb.read(&oid)?;
                        if blob.kind != ObjectKind::Blob {
                            continue;
                        }
                        if blob.data.len() > MAX_ATTR_FILE_BYTES {
                            merged.warnings.push("warning: ignoring overly large gitattributes blob '.gitattributes'".to_string());
                            continue;
                        }
                        let content = String::from_utf8_lossy(&blob.data).into_owned();
                        let display = format!("{path} (tree)");
                        let mut p = parse_gitattributes_content_impl(&content, &display, true);
                        let parent = Path::new(&path)
                            .parent()
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .filter(|s| !s.is_empty());
                        for mut r in p.rules.drain(..) {
                            if let Some(ref pre) = parent {
                                r.pattern = format!("{pre}/{}", r.pattern);
                            }
                            merged.rules.push(r);
                        }
                        merged.macros.defs.extend(p.macros.defs.drain());
                        merged.warnings.append(&mut p.warnings);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Resolve `attr.tree`, `GIT_ATTR_SOURCE`, `--source` precedence for check-attr.
pub fn resolve_attr_treeish(
    repo: &Repository,
    source_arg: Option<&str>,
) -> std::result::Result<Option<String>, crate::error::Error> {
    let env_src = std::env::var("GIT_ATTR_SOURCE")
        .ok()
        .filter(|s| !s.is_empty());
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let cfg_tree = config.get("attr.tree");
    let chosen = source_arg.map(|s| s.to_string()).or(env_src).or(cfg_tree);
    Ok(chosen)
}

/// Parse a revision to a tree OID for attribute loading.
pub fn resolve_tree_oid(repo: &Repository, spec: &str) -> std::result::Result<ObjectId, String> {
    let oid = resolve_revision(repo, spec).map_err(|e| e.to_string())?;
    let obj = repo.odb.read(&oid).map_err(|e| e.to_string())?;
    match obj.kind {
        ObjectKind::Commit => {
            let c = crate::objects::parse_commit(&obj.data).map_err(|e| e.to_string())?;
            Ok(c.tree)
        }
        ObjectKind::Tree => Ok(oid),
        _ => Err("revision is not a commit or tree".to_string()),
    }
}

/// Load attributes from the index (stage 0) for `.gitattributes` paths only.
pub fn load_gitattributes_from_index(
    index: &Index,
    odb: &Odb,
    work_tree: &Path,
) -> std::result::Result<ParsedGitAttributes, crate::error::Error> {
    let mut merged = ParsedGitAttributes::default();
    let mut paths: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0 && e.path.ends_with(b".gitattributes"))
        .map(|e| e.path.clone())
        .collect();
    paths.sort();
    for path_bytes in paths {
        let Ok(rel) = std::str::from_utf8(&path_bytes) else {
            continue;
        };
        let Some(entry) = index.get(&path_bytes, 0) else {
            continue;
        };
        let obj = odb.read(&entry.oid)?;
        if obj.data.len() > MAX_ATTR_FILE_BYTES {
            merged.warnings.push(format!(
                "warning: ignoring overly large gitattributes blob '{}'",
                rel
            ));
            continue;
        }
        let content = String::from_utf8_lossy(&obj.data);
        let mut p = parse_gitattributes_content_impl(&content, rel, true);
        let parent = Path::new(rel).parent().and_then(|p| {
            let s = p.to_str()?;
            if s.is_empty() {
                None
            } else {
                Some(s.replace('\\', "/"))
            }
        });
        for mut r in p.rules.drain(..) {
            if let Some(ref pre) = parent {
                r.pattern = format!("{pre}/{}", r.pattern);
            }
            merged.rules.push(r);
        }
        merged.macros.defs.extend(p.macros.defs.drain());
        merged.warnings.append(&mut p.warnings);
    }
    let _ = work_tree;
    Ok(merged)
}

/// Return `builtin_objectmode` value for a path (working tree), or `None` if unavailable.
#[must_use]
pub fn builtin_objectmode_worktree(repo: &Repository, rel_path: &str) -> Option<String> {
    let wt = repo.work_tree.as_ref()?;
    let p = wt.join(rel_path);
    let meta = fs::symlink_metadata(&p).ok()?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Some("120000".to_string());
    }
    if ft.is_dir() {
        return Some("040000".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = normalize_mode(meta.mode());
        Some(format!("{:06o}", m))
    }
    #[cfg(not(unix))]
    {
        let _ = repo;
        None
    }
}

/// `builtin_objectmode` from the index when `--cached` is used.
#[must_use]
pub fn builtin_objectmode_index(index: &Index, rel_path: &str) -> Option<String> {
    let key = rel_path.as_bytes();
    let e = index.get(key, 0)?;
    let m = e.mode;
    if m == MODE_SYMLINK {
        return Some("120000".to_string());
    }
    if m == MODE_GITLINK {
        return Some("160000".to_string());
    }
    if m == MODE_TREE {
        return Some("040000".to_string());
    }
    if m == MODE_EXECUTABLE {
        return Some("100755".to_string());
    }
    if m == MODE_REGULAR {
        return Some("100644".to_string());
    }
    Some(format!("{:06o}", m))
}
