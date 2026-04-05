//! `grit grep` — search tracked files for a pattern.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use regex::{Regex, RegexBuilder};
use std::io::{self, Write};
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::index::{Index, MODE_GITLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::wildmatch::wildmatch;

/// Arguments for `grit grep`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Show line numbers.
    #[arg(short = 'n', long = "line-number")]
    pub line_number: bool,

    /// Suppress line numbers (overrides -n).
    #[arg(long = "no-line-number", hide = true)]
    pub no_line_number: bool,

    /// Show count of matching lines per file.
    #[arg(short = 'c', long = "count")]
    pub count: bool,

    /// Suppress filename prefix on output.
    #[arg(long = "no-filename")]
    pub no_filename: bool,

    /// Force filename prefix on output.
    #[arg(short = 'H', long = "with-filename")]
    pub with_filename: bool,

    /// Show only filenames with matches.
    #[arg(short = 'l', long = "files-with-matches")]
    pub files_with_matches: bool,

    /// Show only filenames without matches.
    #[arg(short = 'L', long = "files-without-match")]
    pub files_without_match: bool,

    /// Case insensitive matching.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,

    /// Match whole words only.
    #[arg(short = 'w', long = "word-regexp")]
    pub word_regexp: bool,

    /// Process binary files as if they were text.
    #[arg(short = 'a', long = "text")]
    pub text_mode: bool,

    /// Don't match patterns in binary files.
    #[arg(short = 'I')]
    pub ignore_binary: bool,

    /// Invert match (show non-matching lines).
    #[arg(short = 'v', long = "invert-match")]
    pub invert_match: bool,

    /// Explicit pattern (can be used multiple times).
    #[arg(short = 'e', value_name = "PATTERN", allow_hyphen_values = true)]
    pub patterns: Vec<String>,

    /// Use extended regular expressions.
    #[arg(short = 'E', long = "extended-regexp")]
    pub extended_regexp: bool,

    /// Use Perl-compatible regular expressions.
    #[arg(short = 'P', long = "perl-regexp")]
    pub perl_regexp: bool,

    /// Use fixed strings (literal matching, no regex).
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,

    /// Use basic regular expressions (default).
    #[arg(short = 'G', long = "basic-regexp")]
    pub basic_regexp: bool,

    /// Search blobs registered in the index file instead of the work tree.
    #[arg(long = "cached")]
    pub cached: bool,

    /// Read patterns from file, one per line.
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub pattern_file: Option<String>,

    /// Combine patterns with AND (all -e patterns must match).
    #[arg(long = "and")]
    pub and: bool,

    /// Combine patterns with OR (any -e pattern must match) — this is the default.
    #[arg(long = "or")]
    pub or: bool,

    /// Negate the next pattern (can appear multiple times).
    #[arg(long = "not", action = clap::ArgAction::Count)]
    pub not: u8,

    /// Require all patterns to match (line-level AND).
    #[arg(long = "all-match")]
    pub all_match: bool,

    /// Show the whole function as context.
    #[arg(short = 'W', long = "function-context")]
    pub function_context: bool,

    /// Limit matches per file.
    #[arg(
        short = 'm',
        long = "max-count",
        value_name = "NUM",
        allow_negative_numbers = true
    )]
    pub max_count: Option<i64>,

    /// Suppress output; exit with status 0 on match.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Number of threads to use (accepted but ignored).
    #[arg(long = "threads", value_name = "N")]
    pub threads: Option<usize>,

    /// Show column number of first match.
    #[arg(long = "column")]
    pub column: bool,

    /// Show context lines after match.
    #[arg(
        short = 'A',
        long = "after-context",
        value_name = "NUM",
        allow_hyphen_values = true
    )]
    pub after_context: Option<usize>,

    /// Show context lines before match.
    #[arg(
        short = 'B',
        long = "before-context",
        value_name = "NUM",
        allow_hyphen_values = true
    )]
    pub before_context: Option<usize>,

    /// Show context lines before and after match.
    #[arg(
        short = 'C',
        long = "context",
        value_name = "NUM",
        allow_hyphen_values = true
    )]
    pub context: Option<usize>,

    /// Only print the matched parts of a matching line.
    #[arg(short = 'o', long = "only-matching")]
    pub only_matching: bool,

    /// Descend at most <depth> levels of directories.
    #[arg(
        long = "max-depth",
        value_name = "DEPTH",
        allow_negative_numbers = true
    )]
    pub max_depth: Option<i64>,

    /// Recurse into subdirectories (default, same as --max-depth=-1).
    #[arg(long = "recursive", short = 'r')]
    pub recursive: bool,

    /// Do not recurse into subdirectories (same as --max-depth=0).
    #[arg(long = "no-recursive")]
    pub no_recursive: bool,

    /// Show the full path of the file relative to the top-level directory.
    #[arg(long = "full-name")]
    pub full_name: bool,

    /// Print an empty line between matches from different files.
    #[arg(long = "break")]
    pub file_break: bool,

    /// Show the filename above matches from that file instead of prefixing each line.
    #[arg(long = "heading")]
    pub heading: bool,

    /// Use color in output: always, never, auto.
    #[arg(long = "color", value_name = "WHEN", default_value = "never")]
    pub color: String,

    /// Recurse into submodules.
    #[arg(long = "recurse-submodules")]
    pub recurse_submodules: bool,

    /// Do not recurse into submodules (overrides config).
    #[arg(long = "no-recurse-submodules")]
    pub no_recurse_submodules: bool,

    /// Search also in untracked files.
    #[arg(long = "untracked")]
    pub untracked: bool,

    /// Search files not managed by Git (implies --untracked).
    #[arg(long = "no-index")]
    pub no_index: bool,

    /// Use textconv filter (accepted but not yet implemented).
    #[arg(long = "textconv")]
    pub textconv: bool,

    /// Do not use textconv filter.
    #[arg(long = "no-textconv")]
    pub no_textconv: bool,

    /// Positional arguments: [pattern] [<tree>] [-- pathspec...]
    #[arg(trailing_var_arg = true)]
    pub positional: Vec<String>,
}

impl Args {
    fn before_ctx(&self) -> usize {
        self.context.or(self.before_context).unwrap_or(0)
    }
    fn after_ctx(&self) -> usize {
        self.context.or(self.after_context).unwrap_or(0)
    }
    fn use_color(&self) -> bool {
        self.color == "always"
    }
    fn has_context(&self) -> bool {
        self.before_ctx() > 0 || self.after_ctx() > 0
    }
    fn show_line_number(&self) -> bool {
        // effective_line_number is set in run() to account for config
        self.line_number && !self.no_line_number
    }
    fn show_filename(&self) -> bool {
        if self.no_filename {
            return false;
        }
        true // default: show filename
    }
    /// Effective max depth: None means unlimited, Some(n) means limit to depth n.
    fn effective_max_depth(&self) -> Option<usize> {
        if self.no_recursive {
            return Some(0);
        }
        match self.max_depth {
            Some(d) if d < 0 => None, // -1 means unlimited
            Some(d) => Some(d as usize),
            None => None, // default: unlimited
        }
    }
}

/// Run `grit grep`.
pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Apply grep config settings
    {
        let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
        if let Some(ref c) = config {
            // grep.linenumber: if user didn't explicitly pass -n or --no-line-number
            if !args.line_number && !args.no_line_number {
                if let Some(val) = c.get("grep.linenumber") {
                    args.line_number = val == "true" || val == "1" || val == "yes";
                }
            }
            // grep.patternType / grep.extendedRegexp: affect regex mode
            // Only apply config if user didn't explicitly pass -E, -F, -P, or -G
            let user_set_type =
                args.extended_regexp || args.fixed_strings || args.perl_regexp || args.basic_regexp;
            if !user_set_type {
                let mut pattern_type_set = false;
                if let Some(pt) = c
                    .get("grep.patterntype")
                    .or_else(|| c.get("grep.patternType"))
                {
                    match pt.to_lowercase().as_str() {
                        "extended" => {
                            args.extended_regexp = true;
                            pattern_type_set = true;
                        }
                        "fixed" => {
                            args.fixed_strings = true;
                            pattern_type_set = true;
                        }
                        "perl" => {
                            args.perl_regexp = true;
                            pattern_type_set = true;
                        }
                        "basic" => {
                            pattern_type_set = true; /* BRE is default */
                        }
                        "default" => { /* fall through to grep.extendedRegexp */ }
                        _ => {}
                    }
                }
                // grep.extendedRegexp is only consulted if grep.patternType is unset or "default"
                if !pattern_type_set {
                    if let Some(val) = c
                        .get("grep.extendedregexp")
                        .or_else(|| c.get("grep.extendedRegexp"))
                    {
                        if val == "true" || val == "1" || val == "yes" {
                            args.extended_regexp = true;
                        }
                    }
                }
            }
            // Check grep.threads config
            if let Some(val) = c.get("grep.threads") {
                if val != "0" && val != "1" {
                    eprintln!("warning: no threads support, ignoring grep.threads");
                }
            }
            // submodule.recurse config: enable --recurse-submodules if not explicitly set
            if !args.recurse_submodules && !args.no_recurse_submodules {
                if let Some(val) = c.get("submodule.recurse") {
                    if val == "true" || val == "1" || val == "yes" {
                        args.recurse_submodules = true;
                    }
                }
            }
        }
    }

    // --no-recurse-submodules overrides config
    if args.no_recurse_submodules {
        args.recurse_submodules = false;
    }

    // --no-index: ignore --recurse-submodules silently
    if args.no_index {
        args.recurse_submodules = false;
    }

    // Incompatibility checks
    if args.recurse_submodules && args.untracked {
        bail!("option --untracked not supported with --recurse-submodules");
    }

    // Warn about unsupported threading
    if let Some(n) = args.threads {
        if n > 0 {
            eprintln!("warning: no threads support, ignoring --threads");
        }
    }

    // Parse positional arguments: [pattern] [tree-ish] [-- pathspec...]
    let (mut patterns, tree_ish, pathspecs) = parse_positional(&args, &repo)?;

    // Read patterns from file if -f is given
    if let Some(ref pattern_file) = args.pattern_file {
        let content = std::fs::read_to_string(pattern_file)
            .with_context(|| format!("cannot read pattern file: '{pattern_file}'"))?;
        for line in content.lines() {
            if !line.is_empty() {
                patterns.push(line.to_string());
            }
        }
    }

    if patterns.is_empty() {
        bail!("no pattern given");
    }

    // Determine matching mode: --all-match or --and means all patterns must match a line
    let all_match = args.all_match || args.and;

    // Build the regex matchers
    let matchers = build_matchers(&patterns, &args)?;

    let stdout = io::stdout();
    let mut out_handle = stdout.lock();
    let mut sink = io::sink();
    let out: &mut dyn Write = if args.quiet {
        &mut sink
    } else {
        &mut out_handle
    };
    // Tracks whether we need a "--" separator before the next context group
    let mut need_sep = false;
    let all_match = all_match; // shadow to pass into closures

    let found_any;
    if args.no_index {
        // --no-index: walk the filesystem instead of using the index
        let start_dir = std::env::current_dir().context("cannot get current directory")?;
        found_any = grep_filesystem(
            &start_dir,
            "",
            &matchers,
            &args,
            &pathspecs,
            &mut need_sep,
            out,
            all_match,
        )?;
    } else if let Some(tree_spec) = &tree_ish {
        // Search a tree object
        let oid = resolve_revision(&repo, tree_spec)
            .or_else(|_| resolve_ref(&repo.git_dir, tree_spec))
            .or_else(|_| resolve_ref(&repo.git_dir, &format!("refs/heads/{tree_spec}")))
            .with_context(|| format!("not a valid revision: '{tree_spec}'"))?;

        let obj = repo.odb.read(&oid)?;
        let tree_oid = if obj.kind == ObjectKind::Commit {
            let commit = parse_commit(&obj.data)?;
            commit.tree
        } else if obj.kind == ObjectKind::Tree {
            oid
        } else {
            bail!("'{}' is not a tree-ish", tree_spec);
        };

        let tree_obj = repo.odb.read(&tree_oid)?;
        // Load diff attrs: try working tree, then index
        let diff_attrs = if let Some(ref wt) = repo.work_tree {
            let wt_attrs = load_diff_attrs(wt);
            if wt_attrs.is_empty() {
                load_diff_attrs_from_index(&repo)
            } else {
                wt_attrs
            }
        } else {
            load_diff_attrs_from_index(&repo)
        };
        found_any = grep_tree(
            &repo,
            &tree_obj.data,
            "",
            0,
            &matchers,
            &args,
            &pathspecs,
            Some(tree_spec),
            &mut need_sep,
            out,
            all_match,
            &diff_attrs,
        )?;
    } else if args.cached {
        // Search index blobs (--cached)
        found_any = grep_cached(
            &repo,
            "",
            &matchers,
            &args,
            &pathspecs,
            &mut need_sep,
            out,
            all_match,
        )?;
    } else {
        // Search working tree (tracked files from index)
        found_any = grep_worktree(
            &repo,
            "",
            &matchers,
            &args,
            &pathspecs,
            &mut need_sep,
            out,
            all_match,
        )?;
    }

    if found_any {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

/// Grep the index (--cached mode), optionally recursing into submodules.
/// `path_prefix` is prepended to filenames for submodule display (e.g. "submodule/").
fn grep_cached(
    repo: &Repository,
    path_prefix: &str,
    matchers: &[Regex],
    args: &Args,
    pathspecs: &[String],
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    all_match: bool,
) -> Result<bool> {
    let index = Index::load(&repo.index_path()).context("loading index")?;
    // Load diff attrs from index (for --cached, use index attrs) or worktree
    let diff_attrs = if let Some(ref wt) = repo.work_tree {
        // Try worktree first, fallback to index
        let wt_attrs = load_diff_attrs(wt);
        if wt_attrs.is_empty() {
            load_diff_attrs_from_index(repo)
        } else {
            wt_attrs
        }
    } else {
        load_diff_attrs_from_index(repo)
    };
    let mut seen = std::collections::HashSet::new();
    let mut found_any = false;

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        if !seen.insert(path_str.clone()) {
            continue;
        }

        let display_path = format!("{path_prefix}{path_str}");

        // Pathspec filtering is relative to the superproject
        let is_submodule = entry.mode == MODE_GITLINK;
        if !pathspecs.is_empty() && !any_pathspec_matches(&display_path, pathspecs, is_submodule) {
            continue;
        }

        // Submodule entry (gitlink)
        if is_submodule {
            if args.recurse_submodules {
                if let Some(work_tree) = &repo.work_tree {
                    let sub_path = work_tree.join(&path_str);
                    if let Ok(sub_repo) = open_submodule_repo(&sub_path) {
                        if grep_cached(
                            &sub_repo,
                            &format!("{display_path}/"),
                            matchers,
                            args,
                            pathspecs,
                            need_sep,
                            out,
                            all_match,
                        )? {
                            found_any = true;
                        }
                    }
                }
            }
            continue;
        }

        if let Some(max_depth) = args.effective_max_depth() {
            let depth = display_path.matches('/').count();
            if depth > max_depth {
                continue;
            }
        }
        let obj = match repo.odb.read(&entry.oid) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let content_is_binary = obj.data.iter().take(8000).any(|&b| b == 0);
        let binary_override = check_binary_override(&diff_attrs, &path_str);
        let is_binary = match binary_override {
            BinaryOverride::ForceBinary => true,
            BinaryOverride::ForceText => false,
            BinaryOverride::None => content_is_binary,
        };
        if is_binary && args.ignore_binary {
            continue;
        }
        if is_binary && !args.text_mode {
            if args.count || args.files_with_matches || args.files_without_match || args.quiet {
                let content = String::from_utf8_lossy(&obj.data);
                if grep_content(
                    &display_path,
                    &content,
                    matchers,
                    args,
                    None,
                    need_sep,
                    out,
                    all_match,
                )? {
                    found_any = true;
                }
            } else {
                let content = String::from_utf8_lossy(&obj.data);
                let has_match = matchers.iter().any(|re| re.is_match(&content));
                if has_match {
                    writeln!(out, "Binary file {} matches", display_path)?;
                    found_any = true;
                }
            }
        } else {
            let content = String::from_utf8_lossy(&obj.data);
            if grep_content(
                &display_path,
                &content,
                matchers,
                args,
                None,
                need_sep,
                out,
                all_match,
            )? {
                found_any = true;
            }
        }
    }
    Ok(found_any)
}

/// Grep the working tree, optionally recursing into submodules.
/// `path_prefix` is prepended to filenames for submodule display.
fn grep_worktree(
    repo: &Repository,
    path_prefix: &str,
    matchers: &[Regex],
    args: &Args,
    pathspecs: &[String],
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    all_match: bool,
) -> Result<bool> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot grep in bare repository"))?;

    let index = Index::load(&repo.index_path()).context("loading index")?;
    let diff_attrs = load_diff_attrs(work_tree);
    let mut seen = std::collections::HashSet::new();
    let mut found_any = false;

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        if !seen.insert(path_str.clone()) {
            continue;
        }

        let display_path = format!("{path_prefix}{path_str}");

        // Pathspec filtering is relative to the superproject
        let is_submodule = entry.mode == MODE_GITLINK;
        if !pathspecs.is_empty() && !any_pathspec_matches(&display_path, pathspecs, is_submodule) {
            continue;
        }

        // Submodule entry (gitlink)
        if is_submodule {
            if args.recurse_submodules {
                let sub_path = work_tree.join(&path_str);
                if let Ok(sub_repo) = open_submodule_repo(&sub_path) {
                    if grep_worktree(
                        &sub_repo,
                        &format!("{display_path}/"),
                        matchers,
                        args,
                        pathspecs,
                        need_sep,
                        out,
                        all_match,
                    )? {
                        found_any = true;
                    }
                }
            }
            continue;
        }

        // Apply max-depth filter
        if let Some(max_depth) = args.effective_max_depth() {
            let depth = display_path.matches('/').count();
            if depth > max_depth {
                continue;
            }
        }

        let full_path = work_tree.join(&path_str);
        let content = match std::fs::read(&full_path) {
            Ok(c) => c,
            Err(_) => continue, // deleted but still in index
        };

        let content_is_binary = content.iter().take(8000).any(|&b| b == 0);
        // Apply diff attribute override
        let binary_override = check_binary_override(&diff_attrs, &path_str);
        let is_binary = match binary_override {
            BinaryOverride::ForceBinary => true,
            BinaryOverride::ForceText => false,
            BinaryOverride::None => content_is_binary,
        };

        if is_binary && args.ignore_binary {
            continue;
        }

        if is_binary && !args.text_mode {
            if args.count || args.files_with_matches || args.files_without_match || args.quiet {
                let content_str = String::from_utf8_lossy(&content);
                if grep_content(
                    &display_path,
                    &content_str,
                    matchers,
                    args,
                    None,
                    need_sep,
                    out,
                    all_match,
                )? {
                    found_any = true;
                }
            } else {
                let content_str = String::from_utf8_lossy(&content);
                let has_match = matchers.iter().any(|re| re.is_match(&content_str));
                if has_match {
                    writeln!(out, "Binary file {} matches", display_path)?;
                    found_any = true;
                }
            }
        } else {
            let content_str = String::from_utf8_lossy(&content);
            if grep_content(
                &display_path,
                &content_str,
                matchers,
                args,
                None,
                need_sep,
                out,
                all_match,
            )? {
                found_any = true;
            }
        }
    }
    Ok(found_any)
}

/// Check if a pathspec contains glob special characters.
fn has_glob_chars(s: &str) -> bool {
    s.bytes().any(|b| matches!(b, b'*' | b'?' | b'[' | b'\\'))
}

/// Check if a path matches a pathspec. Handles both plain prefix matching
/// and glob/wildmatch patterns.
fn matches_pathspec(path: &str, pathspec: &str, is_dir: bool) -> bool {
    if has_glob_chars(pathspec) {
        // Use wildmatch for glob patterns.
        // Git pathspec wildcards: `*` matches `/` (no WM_PATHNAME),
        if wildmatch(pathspec.as_bytes(), path.as_bytes(), 0) {
            return true;
        }
        if is_dir {
            // Check if the pathspec could match children of this dir.
            // For glob pathspecs, if `path/` is a prefix that the pattern
            // could match through, we should descend.
            // Strategy: check if pathspec matches path + "/<anything>" by
            // testing if the pattern matches a synthetic child.
            // Use a simple check: see if pathspec starts with the dir
            // path literally (before any glob chars).
            let literal_prefix = pathspec
                .find(['*', '?', '[', '\\'])
                .map(|pos| &pathspec[..pos])
                .unwrap_or(pathspec);
            // If the literal prefix starts with path/ then this dir is needed
            if literal_prefix.starts_with(&format!("{path}/")) {
                return true;
            }
            // If path starts with the literal prefix (stripped of trailing /),
            // and the next char in pathspec is a glob, descend.
            let lp_trimmed = literal_prefix.trim_end_matches('/');
            if !lp_trimmed.is_empty() && path.starts_with(lp_trimmed) {
                return true;
            }
            // Also try: if pathspec has directory separators, match dir parts
            // against path parts. E.g. "submodul?/a" should match dir "submodule".
            for (i, _) in pathspec.match_indices('/') {
                let ps_dir = &pathspec[..i];
                if wildmatch(ps_dir.as_bytes(), path.as_bytes(), 0) {
                    return true;
                }
            }
        }
        false
    } else {
        // Plain prefix matching
        path == pathspec
            || path.starts_with(&format!("{pathspec}/"))
            || (is_dir && pathspec.starts_with(&format!("{path}/")))
    }
}

/// Check if any pathspec matches a path.
fn any_pathspec_matches(path: &str, pathspecs: &[String], is_dir: bool) -> bool {
    pathspecs.iter().any(|p| matches_pathspec(path, p, is_dir))
}

/// Binary override from .gitattributes diff attribute.
/// `ForceBinary` means the file has `-diff` (treat as binary).
/// `ForceText` means the file has `diff` set (treat as text).
/// `None` means no override.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BinaryOverride {
    ForceBinary,
    ForceText,
    None,
}

/// A parsed gitattributes rule for the diff attribute.
struct DiffAttrRule {
    pattern: String,
    is_negated: bool, // "-diff" → treat as binary
                      // If not negated, treat as text
}

/// Load diff attribute rules from .gitattributes files.
fn load_diff_attrs(work_tree: &Path) -> Vec<DiffAttrRule> {
    let mut rules = Vec::new();
    // Load root .gitattributes
    let root = work_tree.join(".gitattributes");
    if root.exists() {
        if let Ok(content) = std::fs::read_to_string(&root) {
            parse_diff_attrs(&content, &mut rules);
        }
    }
    // Load .git/info/attributes
    let info = work_tree.join(".git/info/attributes");
    if info.exists() {
        if let Ok(content) = std::fs::read_to_string(&info) {
            parse_diff_attrs(&content, &mut rules);
        }
    }
    rules
}

/// Load diff attribute rules from .gitattributes in the index.
fn load_diff_attrs_from_index(repo: &Repository) -> Vec<DiffAttrRule> {
    let mut rules = Vec::new();
    if let Ok(index) = Index::load(&repo.index_path()) {
        if let Some(entry) = index.entries.iter().find(|e| e.path == b".gitattributes") {
            if let Ok(obj) = repo.odb.read(&entry.oid) {
                if let Ok(content) = String::from_utf8(obj.data) {
                    parse_diff_attrs(&content, &mut rules);
                }
            }
        }
    }
    // Also check .git/info/attributes
    if let Some(ref wt) = repo.work_tree {
        let info = wt.join(".git/info/attributes");
        if info.exists() {
            if let Ok(content) = std::fs::read_to_string(&info) {
                parse_diff_attrs(&content, &mut rules);
            }
        }
    }
    // Also try git_dir/info/attributes
    let info2 = repo.git_dir.join("info/attributes");
    if info2.exists() {
        if let Ok(content) = std::fs::read_to_string(&info2) {
            parse_diff_attrs(&content, &mut rules);
        }
    }
    rules
}

fn parse_diff_attrs(content: &str, rules: &mut Vec<DiffAttrRule>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let pattern = match parts.next() {
            Some(p) => p.to_owned(),
            None => continue,
        };
        for part in parts {
            if part == "-diff" {
                rules.push(DiffAttrRule {
                    pattern,
                    is_negated: true,
                });
                break;
            } else if part == "diff" {
                rules.push(DiffAttrRule {
                    pattern,
                    is_negated: false,
                });
                break;
            } else if part.starts_with("diff=") {
                // diff=<driver> — treat as text
                rules.push(DiffAttrRule {
                    pattern,
                    is_negated: false,
                });
                break;
            }
        }
    }
}

/// Check the diff attribute for a file path.
fn check_binary_override(rules: &[DiffAttrRule], path: &str) -> BinaryOverride {
    let mut result = BinaryOverride::None;
    let basename = path.rsplit('/').next().unwrap_or(path);
    for rule in rules {
        // If pattern has no slash, match against basename
        let matches = if rule.pattern.contains('/') {
            wildmatch(rule.pattern.as_bytes(), path.as_bytes(), 0)
        } else {
            wildmatch(rule.pattern.as_bytes(), basename.as_bytes(), 0)
        };
        if matches {
            result = if rule.is_negated {
                BinaryOverride::ForceBinary
            } else {
                BinaryOverride::ForceText
            };
        }
    }
    result
}

/// Grep the filesystem recursively (--no-index mode).
fn grep_filesystem(
    dir: &Path,
    prefix: &str,
    matchers: &[Regex],
    args: &Args,
    pathspecs: &[String],
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    all_match: bool,
) -> Result<bool> {
    let mut found_any = false;
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return Ok(false),
    };
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Skip .git directories
        if name_str == ".git" {
            continue;
        }
        let display_path = if prefix.is_empty() {
            name_str.to_string()
        } else {
            format!("{prefix}/{name_str}")
        };

        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if ft.is_dir() {
            if grep_filesystem(
                &entry.path(),
                &display_path,
                matchers,
                args,
                pathspecs,
                need_sep,
                out,
                all_match,
            )? {
                found_any = true;
            }
        } else if ft.is_file() {
            // Apply pathspec filter
            if !pathspecs.is_empty() && !any_pathspec_matches(&display_path, pathspecs, false) {
                continue;
            }

            let content = match std::fs::read(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let is_binary = content.iter().take(8000).any(|&b| b == 0);
            if is_binary && args.ignore_binary {
                continue;
            }
            if is_binary && !args.text_mode {
                let content_str = String::from_utf8_lossy(&content);
                let has_match = matchers.iter().any(|re| re.is_match(&content_str));
                if has_match {
                    writeln!(out, "Binary file {} matches", display_path)?;
                    found_any = true;
                }
            } else {
                let content_str = String::from_utf8_lossy(&content);
                if grep_content(
                    &display_path,
                    &content_str,
                    matchers,
                    args,
                    None,
                    need_sep,
                    out,
                    all_match,
                )? {
                    found_any = true;
                }
            }
        }
    }
    Ok(found_any)
}

/// Open a submodule repository from its working directory path.
fn open_submodule_repo(sub_path: &Path) -> Result<Repository> {
    let git_path = sub_path.join(".git");
    if git_path.is_dir() {
        // Regular .git directory
        Repository::open(&git_path, Some(sub_path)).map_err(|e| {
            anyhow::anyhow!("failed to open submodule at {}: {}", sub_path.display(), e)
        })
    } else if git_path.is_file() {
        // gitdir: file pointing to the actual git directory
        let content = std::fs::read_to_string(&git_path)
            .with_context(|| format!("failed to read {}", git_path.display()))?;
        let gitdir = content
            .trim()
            .strip_prefix("gitdir: ")
            .ok_or_else(|| anyhow::anyhow!("invalid .git file in {}", sub_path.display()))?;
        let gitdir_path = if Path::new(gitdir).is_absolute() {
            std::path::PathBuf::from(gitdir)
        } else {
            sub_path.join(gitdir)
        };
        let gitdir_path = gitdir_path
            .canonicalize()
            .with_context(|| format!("failed to resolve gitdir {}", gitdir_path.display()))?;
        Repository::open(&gitdir_path, Some(sub_path)).map_err(|e| {
            anyhow::anyhow!("failed to open submodule at {}: {}", sub_path.display(), e)
        })
    } else {
        anyhow::bail!("no .git directory in {}", sub_path.display())
    }
}

/// Try to resolve a string as a revision (commit/tree).
fn is_revision(repo: &Repository, spec: &str) -> bool {
    let oid = resolve_revision(repo, spec)
        .or_else(|_| resolve_ref(&repo.git_dir, spec))
        .or_else(|_| resolve_ref(&repo.git_dir, &format!("refs/heads/{spec}")));
    match oid {
        Ok(oid) => {
            // Verify the OID is readable and is a commit or tree (not a blob)
            match repo.odb.read(&oid) {
                Ok(obj) => matches!(obj.kind, ObjectKind::Commit | ObjectKind::Tree),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Parse positional arguments into (patterns, tree_ish, pathspecs).
fn parse_positional(
    args: &Args,
    repo: &Repository,
) -> Result<(Vec<String>, Option<String>, Vec<String>)> {
    let mut patterns = args.patterns.clone();
    let positional = &args.positional;

    // Find `--` separator
    let sep_pos = positional.iter().position(|a| a == "--");

    let (before_sep, pathspecs) = match sep_pos {
        Some(pos) => (&positional[..pos], positional[pos + 1..].to_vec()),
        None => (positional.as_slice(), Vec::new()),
    };

    let mut tree_ish = None;

    if patterns.is_empty() {
        if before_sep.is_empty() {
            return Ok((patterns, tree_ish, pathspecs));
        }
        patterns.push(before_sep[0].clone());
        let rest = &before_sep[1..];
        if !rest.is_empty() && is_revision(repo, &rest[0]) {
            tree_ish = Some(rest[0].clone());
            let mut ps = pathspecs;
            ps.extend(rest[1..].iter().cloned());
            return Ok((patterns, tree_ish, ps));
        }
        let mut ps = pathspecs;
        ps.extend(rest.iter().cloned());
        return Ok((patterns, tree_ish, ps));
    }

    if !before_sep.is_empty() && is_revision(repo, &before_sep[0]) {
        tree_ish = Some(before_sep[0].clone());
        let mut ps = pathspecs;
        ps.extend(before_sep[1..].iter().cloned());
        return Ok((patterns, tree_ish, ps));
    }

    let mut ps = pathspecs;
    ps.extend(before_sep.iter().cloned());
    Ok((patterns, tree_ish, ps))
}

/// Build regex matchers from patterns.
/// Convert a BRE (basic regular expression) pattern to an ERE-compatible pattern
/// for the Rust regex crate. In BRE, +, ?, {, }, (, ), | are literal and their
/// backslash-escaped forms are special. In ERE/Rust regex, they're special without backslash.
/// Convert a BRE (basic regular expression) pattern to an ERE-compatible pattern
/// for the Rust regex crate. In BRE, +, ?, {, }, (, ), | are literal and their
/// backslash-escaped forms are special. In ERE/Rust regex, they're special without backslash.
fn bre_to_ere(pat: &str) -> String {
    let mut result = String::with_capacity(pat.len());
    let chars: Vec<char> = pat.chars().collect();
    let mut i = 0;
    let mut in_bracket = false;
    while i < chars.len() {
        if in_bracket {
            // Inside [...], most things are literal
            if chars[i] == ']' && i > 0 {
                result.push(']');
                in_bracket = false;
                i += 1;
            } else if chars[i] == '\\' && i + 1 < chars.len() {
                // In BRE char class, \ is literal. But Rust regex treats
                // \d, \w, \s etc. as shorthand classes inside [...].
                // Emit both chars as literals: \\\\ + next char
                let next = chars[i + 1];
                if next.is_ascii_alphabetic() {
                    // Escape the backslash so Rust regex sees it as literal
                    result.push('\\');
                    result.push('\\');
                    result.push(next);
                } else {
                    result.push('\\');
                    result.push(next);
                }
                i += 2;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '[' {
            result.push('[');
            in_bracket = true;
            i += 1;
            // Handle [^ or [! negation, and ] as first char
            if i < chars.len() && (chars[i] == '^' || chars[i] == '!') {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                result.push(']');
                i += 1;
            }
        } else if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                '+' | '?' | '{' | '}' | '(' | ')' | '|' => {
                    // \+ in BRE means special +; in ERE just use +
                    result.push(chars[i + 1]);
                    i += 2;
                }
                _ => {
                    result.push(chars[i]);
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else if matches!(chars[i], '+' | '?' | '{' | '}' | '(' | ')' | '|') {
            // Literal in BRE — escape them for ERE
            result.push('\\');
            result.push(chars[i]);
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Fix character classes for Rust regex compatibility.
/// In POSIX (both BRE and ERE), `\d` inside `[...]` means literal `\` and `d`.
/// In Rust regex, `\d` inside `[...]` means digit shorthand.
/// This function escapes backslashes inside character classes.
fn fix_charclass_escapes(pat: &str) -> String {
    let mut result = String::with_capacity(pat.len());
    let chars: Vec<char> = pat.chars().collect();
    let mut i = 0;
    let mut in_bracket = false;
    while i < chars.len() {
        if in_bracket {
            if chars[i] == ']' {
                result.push(']');
                in_bracket = false;
                i += 1;
            } else if chars[i] == '\\' && i + 1 < chars.len() {
                let next = chars[i + 1];
                if next.is_ascii_alphabetic() {
                    // Escape the backslash so Rust regex sees it as literal
                    result.push('\\');
                    result.push('\\');
                    result.push(next);
                } else {
                    result.push('\\');
                    result.push(next);
                }
                i += 2;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '[' {
            result.push('[');
            in_bracket = true;
            i += 1;
            // Handle [^ or [! negation, and ] as first char
            if i < chars.len() && (chars[i] == '^' || chars[i] == '!') {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                result.push(']');
                i += 1;
            }
        } else if chars[i] == '\\' && i + 1 < chars.len() {
            // Outside brackets, pass through
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn build_matchers(patterns: &[String], args: &Args) -> Result<Vec<Regex>> {
    let mut matchers = Vec::new();
    let use_bre = !args.extended_regexp && !args.fixed_strings && !args.perl_regexp;
    for pat in patterns {
        let effective = if args.fixed_strings {
            regex::escape(pat)
        } else if use_bre {
            bre_to_ere(pat)
        } else if args.perl_regexp {
            // Perl regex: pass through as-is (Rust regex handles most PCRE)
            pat.clone()
        } else {
            // ERE: fix character class escapes for Rust regex compatibility
            fix_charclass_escapes(pat)
        };
        let effective = if args.word_regexp {
            format!(r"\b{effective}\b")
        } else {
            effective
        };
        let re = RegexBuilder::new(&effective)
            .case_insensitive(args.ignore_case)
            .build()
            .with_context(|| format!("invalid pattern: '{pat}'"))?;
        matchers.push(re);
    }
    Ok(matchers)
}

// Color constants
const COLOR_FILENAME: &str = "\x1b[35m";
const COLOR_LINENO: &str = "\x1b[32m";
const COLOR_COLUMNNO: &str = "\x1b[32m";
const COLOR_MATCH: &str = "\x1b[1;31m";
const COLOR_SEP: &str = "\x1b[36m";
const COLOR_RESET: &str = "\x1b[m";

/// Colorize all matches in a line.
fn colorize_matches(line: &str, matchers: &[Regex]) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for re in matchers {
        for m in re.find_iter(line) {
            ranges.push((m.start(), m.end()));
        }
    }
    if ranges.is_empty() {
        return line.to_string();
    }
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    let mut result = String::new();
    let mut pos = 0;
    for (s, e) in merged {
        result.push_str(&line[pos..s]);
        result.push_str(COLOR_MATCH);
        result.push_str(&line[s..e]);
        result.push_str(COLOR_RESET);
        pos = e;
    }
    result.push_str(&line[pos..]);
    result
}

fn sep_char(ch: char, color: bool) -> String {
    if color {
        format!("{COLOR_SEP}{ch}{COLOR_RESET}")
    } else {
        ch.to_string()
    }
}

fn fmt_name(name: &str, color: bool) -> String {
    if name.is_empty() {
        return String::new();
    }
    if color {
        format!("{COLOR_FILENAME}{name}{COLOR_RESET}")
    } else {
        name.to_string()
    }
}

/// Build the filename prefix with separator. Returns empty pair when name is empty.
fn name_prefix(name: &str, sep: char, color: bool) -> String {
    if name.is_empty() {
        return String::new();
    }
    let mut s = fmt_name(name, color);
    s.push_str(&sep_char(sep, color));
    s
}

fn fmt_num(n: usize, color: bool) -> String {
    if color {
        format!("{COLOR_LINENO}{n}{COLOR_RESET}")
    } else {
        n.to_string()
    }
}

fn fmt_col(n: usize, color: bool) -> String {
    if color {
        format!("{COLOR_COLUMNNO}{n}{COLOR_RESET}")
    } else {
        n.to_string()
    }
}

/// Get column (1-based) of first match in a line.
fn first_match_col(line: &str, matchers: &[Regex]) -> Option<usize> {
    let mut earliest: Option<usize> = None;
    for re in matchers {
        if let Some(m) = re.find(line) {
            let col = m.start() + 1;
            earliest = Some(earliest.map_or(col, |e: usize| e.min(col)));
        }
    }
    earliest
}

/// Search content of a single file. Returns true if any match found.
/// `need_sep` tracks whether a "--" separator should be printed before the next context group.
fn grep_content(
    filename: &str,
    content: &str,
    matchers: &[Regex],
    args: &Args,
    tree_prefix: Option<&str>,
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    all_match: bool,
) -> Result<bool> {
    let color = args.use_color();

    let display_name = match tree_prefix {
        Some(p) => format!("{p}:{filename}"),
        None => filename.to_string(),
    };
    let show_name = args.show_filename();
    // When suppressing filenames, use empty display_name so prefix is omitted
    let display_name = if show_name {
        display_name
    } else {
        String::new()
    };

    let lines: Vec<&str> = content.lines().collect();
    let nlines = lines.len();
    let before = args.before_ctx();
    let after = args.after_ctx();
    let use_context = args.has_context();

    // Collect matching line indices
    let mut match_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line_matches = if all_match {
            matchers.iter().all(|re| re.is_match(line))
        } else {
            matchers.iter().any(|re| re.is_match(line))
        };
        let effective_match = if args.invert_match {
            !line_matches
        } else {
            line_matches
        };
        if effective_match {
            match_indices.push(i);
        }
    }

    // Apply --max-count: truncate matches (negative means no limit)
    if let Some(max) = args.max_count {
        if max >= 0 {
            match_indices.truncate(max as usize);
        }
    }

    let has_match = !match_indices.is_empty();
    let match_count = match_indices.len() as u64;

    // For --heading mode, we print the filename once and then suppress it in line output
    let display_name = if args.heading && has_match && show_name {
        // Heading will be printed below; use empty display_name for per-line output
        String::new()
    } else {
        display_name
    };

    // Special modes: files-with-matches, files-without-match, count
    if args.files_with_matches {
        if has_match {
            writeln!(out, "{}", fmt_name(&display_name, color))?;
        }
        return Ok(has_match);
    }

    if args.files_without_match {
        if !has_match {
            writeln!(out, "{}", fmt_name(&display_name, color))?;
            return Ok(true);
        }
        return Ok(false);
    }

    if args.count {
        if match_count > 0 {
            writeln!(
                out,
                "{}{}",
                name_prefix(&display_name, ':', color),
                match_count
            )?;
        }
        return Ok(has_match);
    }

    if !has_match {
        return Ok(false);
    }

    // --break: print empty line between file groups
    if args.file_break && *need_sep {
        writeln!(out)?;
    }

    // --heading: print filename once above the matches
    if args.heading && show_name {
        let heading_name = match tree_prefix {
            Some(p) => format!("{p}:{filename}"),
            None => filename.to_string(),
        };
        writeln!(out, "{}", fmt_name(&heading_name, color))?;
    }

    // --only-matching
    if args.only_matching {
        for &idx in &match_indices {
            let line = lines[idx];
            // For each matcher, iterate over matches tracking column
            // as git does: accumulate cno from initial col+1 then add rm_eo
            // after each match (git grep.c show_line logic).
            for re in matchers {
                // Find the first match to get the initial column.
                let first_m = re.find(line);
                if first_m.is_none() {
                    continue;
                }
                let first_m = first_m.unwrap();
                let mut cno = first_m.start() + 1; // 1-indexed initial column
                let mut remaining = &line[first_m.start()..];

                // Output first match
                {
                    let matched_text = first_m.as_str();
                    let mut prefix_str = name_prefix(&display_name, ':', color);
                    if args.show_line_number() {
                        prefix_str.push_str(&fmt_num(idx + 1, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if args.column {
                        prefix_str.push_str(&fmt_col(cno, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if color {
                        writeln!(out, "{prefix_str}{COLOR_MATCH}{matched_text}{COLOR_RESET}")?;
                    } else {
                        writeln!(out, "{prefix_str}{matched_text}")?;
                    }
                }

                // Advance past first match and find subsequent matches
                let match_len = first_m.end() - first_m.start();
                cno += match_len;
                remaining = &remaining[match_len..];

                loop {
                    let next_m = re.find(remaining);
                    if next_m.is_none() {
                        break;
                    }
                    let next_m = next_m.unwrap();
                    let matched_text = next_m.as_str();
                    let col = cno + next_m.start();
                    let mut prefix_str = name_prefix(&display_name, ':', color);
                    if args.show_line_number() {
                        prefix_str.push_str(&fmt_num(idx + 1, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if args.column {
                        prefix_str.push_str(&fmt_col(col, color));
                        prefix_str.push_str(&sep_char(':', color));
                    }
                    if color {
                        writeln!(out, "{prefix_str}{COLOR_MATCH}{matched_text}{COLOR_RESET}")?;
                    } else {
                        writeln!(out, "{prefix_str}{matched_text}")?;
                    }
                    let advance = next_m.end();
                    cno += advance;
                    remaining = &remaining[advance..];
                }
            }
        }
        return Ok(true);
    }

    // Context mode
    if use_context {
        // Build groups of (start, end) ranges
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for &idx in &match_indices {
            let start = idx.saturating_sub(before);
            let end = (idx + after).min(nlines - 1);
            if let Some(last) = groups.last_mut() {
                if start <= last.1 + 1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            groups.push((start, end));
        }

        let match_set: std::collections::HashSet<usize> = match_indices.iter().copied().collect();

        for &(start, end) in &groups {
            // Print separator between groups (including across files)
            if *need_sep {
                writeln!(out, "--")?;
            }
            *need_sep = true;

            for i in start..=end {
                let is_match_line = match_set.contains(&i);
                let separator = if is_match_line { ':' } else { '-' };
                let mut prefix_str = name_prefix(&display_name, separator, color);
                if args.show_line_number() {
                    prefix_str.push_str(&fmt_num(i + 1, color));
                    prefix_str.push_str(&sep_char(separator, color));
                }
                if args.column && is_match_line {
                    if let Some(col) = first_match_col(lines[i], matchers) {
                        prefix_str.push_str(&fmt_col(col, color));
                        prefix_str.push_str(&sep_char(separator, color));
                    }
                }
                if color && is_match_line {
                    writeln!(out, "{prefix_str}{}", colorize_matches(lines[i], matchers))?;
                } else {
                    writeln!(out, "{prefix_str}{}", lines[i])?;
                }
            }
        }
    } else {
        // No context — just print matching lines
        for &idx in &match_indices {
            let line = lines[idx];
            let mut prefix_str = name_prefix(&display_name, ':', color);
            if args.show_line_number() {
                prefix_str.push_str(&fmt_num(idx + 1, color));
                prefix_str.push_str(&sep_char(':', color));
            }
            if args.column {
                let col = if args.invert_match {
                    Some(1) // non-matching lines default to column 1
                } else {
                    first_match_col(line, matchers)
                };
                if let Some(col) = col {
                    prefix_str.push_str(&fmt_col(col, color));
                    prefix_str.push_str(&sep_char(':', color));
                }
            }
            if color {
                writeln!(out, "{prefix_str}{}", colorize_matches(line, matchers))?;
            } else {
                writeln!(out, "{prefix_str}{line}")?;
            }
        }
    }

    *need_sep = true;
    Ok(true)
}

/// Recursively search a tree object.
fn grep_tree(
    repo: &Repository,
    tree_data: &[u8],
    prefix: &str,
    depth: usize,
    matchers: &[Regex],
    args: &Args,
    pathspecs: &[String],
    tree_name: Option<&str>,
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    all_match: bool,
    diff_attrs: &[DiffAttrRule],
) -> Result<bool> {
    let entries = parse_tree(tree_data)?;
    let mut found = false;

    for entry in &entries {
        let name = String::from_utf8_lossy(&entry.name);
        let full_name = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let is_tree = entry.mode == 0o040000;
        let is_gitlink = entry.mode == 0o160000;

        // Apply pathspec filter
        if !pathspecs.is_empty()
            && !any_pathspec_matches(&full_name, pathspecs, is_tree || is_gitlink) {
                continue;
            }

        // Submodule (gitlink) in tree: recurse if --recurse-submodules
        if is_gitlink {
            if args.recurse_submodules {
                if let Some(work_tree) = &repo.work_tree {
                    // Use just the entry name relative to this tree level, not full_name
                    let local_name = name.to_string();
                    let sub_path = work_tree.join(&local_name);
                    if let Ok(sub_repo) = open_submodule_repo(&sub_path) {
                        // The entry.oid is the commit SHA of the submodule
                        let sub_obj = match sub_repo.odb.read(&entry.oid) {
                            Ok(o) => o,
                            Err(_) => continue,
                        };
                        let sub_tree_oid = if sub_obj.kind == ObjectKind::Commit {
                            match parse_commit(&sub_obj.data) {
                                Ok(c) => c.tree,
                                Err(_) => continue,
                            }
                        } else {
                            continue;
                        };
                        let sub_tree_obj = match sub_repo.odb.read(&sub_tree_oid) {
                            Ok(o) => o,
                            Err(_) => continue,
                        };
                        // Load diff attrs for the submodule
                        let sub_diff_attrs = if let Some(ref swt) = sub_repo.work_tree {
                            load_diff_attrs(swt)
                        } else {
                            vec![]
                        };
                        if grep_tree(
                            &sub_repo,
                            &sub_tree_obj.data,
                            &full_name,
                            0,
                            matchers,
                            args,
                            pathspecs,
                            tree_name,
                            need_sep,
                            out,
                            all_match,
                            &sub_diff_attrs,
                        )? {
                            found = true;
                        }
                    }
                }
            }
            continue;
        }

        if is_tree {
            if let Some(max_depth) = args.effective_max_depth() {
                if depth >= max_depth {
                    continue;
                }
            }
            let sub_obj = repo.odb.read(&entry.oid)?;
            if grep_tree(
                repo,
                &sub_obj.data,
                &full_name,
                depth + 1,
                matchers,
                args,
                pathspecs,
                tree_name,
                need_sep,
                out,
                all_match,
                diff_attrs,
            )? {
                found = true;
            }
        } else {
            if let Some(max_depth) = args.effective_max_depth() {
                let file_depth = full_name.matches('/').count();
                if file_depth > max_depth {
                    continue;
                }
            }

            let obj = match repo.odb.read(&entry.oid) {
                Ok(o) => o,
                Err(_) => continue,
            };

            let content_is_binary = obj.data.iter().take(8000).any(|&b| b == 0);
            let binary_override = check_binary_override(diff_attrs, &full_name);
            let is_binary = match binary_override {
                BinaryOverride::ForceBinary => true,
                BinaryOverride::ForceText => false,
                BinaryOverride::None => content_is_binary,
            };

            if is_binary && args.ignore_binary {
                continue;
            }

            if is_binary && !args.text_mode {
                let content = String::from_utf8_lossy(&obj.data);
                let has_match = matchers.iter().any(|re| re.is_match(&content));
                if has_match {
                    if !args.quiet {
                        let display = match tree_name {
                            Some(t) => format!("{t}:{full_name}"),
                            None => full_name.clone(),
                        };
                        writeln!(out, "Binary file {} matches", display)?;
                    }
                    found = true;
                }
            } else {
                let content = String::from_utf8_lossy(&obj.data);
                if grep_content(
                    &full_name, &content, matchers, args, tree_name, need_sep, out, all_match,
                )? {
                    found = true;
                }
            }
        }
    }

    Ok(found)
}
