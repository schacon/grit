//! `grit grep` — search tracked files for a pattern.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use regex::{Regex, RegexBuilder};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::commands::grep_expr::{
    collect_atom_indices, line_matches_expr, match_expr_eval, CompiledGrep, GrepExpr,
};
use crate::commands::grep_pattern::PatternToken;
use crate::explicit_exit::ExplicitExit;
use grit_lib::attributes::quote_path_for_check_attr;
use grit_lib::config::ConfigSet;
use grit_lib::index::{MODE_GITLINK, MODE_TREE};
use grit_lib::objects::{parse_commit, parse_tree, ObjectKind};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{discover_optional, resolve_revision};
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

    /// Use NUL as filename delimiter in output.
    #[arg(short = 'z', long = "null")]
    pub null_following_name: bool,

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

    /// Set by `main` after stripping `-O` / `--open-files-in-pager` (Git does not consume the next argv as the pager unless it is glued: `-Opager`).
    #[arg(skip)]
    pub open_in_pager: bool,

    /// Explicit pager from `-Opager` or `--open-files-in-pager=pager`. When `None` with `open_in_pager`, resolve like Git (GIT_PAGER / config / PAGER).
    #[arg(skip)]
    pub open_pager_cmd: Option<String>,
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

    fn sep_byte(&self) -> u8 {
        if self.null_following_name {
            0
        } else {
            b':'
        }
    }
}

/// Strip `-O` / `--open-files-in-pager` from argv before clap parsing.
///
/// Git only takes the pager from the same argv cell as `-O` (`-Opager`). A separate token after
/// lone `-O` is not the pager (it is usually the pattern).
pub fn preprocess_open_in_pager_argv(argv: Vec<String>) -> (Vec<String>, bool, Option<String>) {
    let mut open_in_pager = false;
    let mut open_pager_cmd: Option<String> = None;
    let mut out = Vec::with_capacity(argv.len());
    for a in argv {
        if a == "-O" || a == "--open-files-in-pager" {
            open_in_pager = true;
            continue;
        }
        if let Some(v) = a.strip_prefix("--open-files-in-pager=") {
            open_in_pager = true;
            if !v.is_empty() {
                open_pager_cmd = Some(v.to_string());
            }
            continue;
        }
        if a.len() >= 2 {
            let b = a.as_bytes();
            if b[0] == b'-' && b[1] == b'O' {
                open_in_pager = true;
                if a.len() > 2 {
                    open_pager_cmd = Some(a[2..].to_string());
                }
                continue;
            }
        }
        out.push(a);
    }
    (out, open_in_pager, open_pager_cmd)
}

/// Entry point after `main` peels pattern-expression tokens from argv.
pub fn run_with_pattern_tokens(pattern_tokens: Vec<PatternToken>, args: Args) -> Result<()> {
    run_inner(pattern_tokens, args)
}

/// Run `grit grep` (no pre-parsed pattern tokens).
pub fn run(args: Args) -> Result<()> {
    run_inner(Vec::new(), args)
}

fn run_inner(pattern_tokens: Vec<PatternToken>, mut args: Args) -> Result<()> {
    let has_attr_src = std::env::var("GIT_ATTR_SOURCE")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some();
    if has_attr_src {
        let repo_opt = discover_optional(None)?;
        if repo_opt.is_none() {
            bail!("fatal: cannot use --attr-source or GIT_ATTR_SOURCE without repo");
        }
    }
    let repo = Repository::discover(None).context("not a git repository")?;

    let config = ConfigSet::load(Some(&repo.git_dir), true).ok();

    // Apply grep config settings
    {
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

    // Positional: [pattern] [tree-ish] [-- pathspec...] — pattern only when no peeled `-e`/`-f`.
    let has_peeled_patterns = !pattern_tokens.is_empty();
    let (first_pos_pattern, tree_ish, mut pathspecs) =
        parse_positional(&args, &repo, has_peeled_patterns)?;
    pathspecs = pathspecs_relative_to_cwd(&repo, &pathspecs);

    let mut pattern_tokens = pattern_tokens;
    if pattern_tokens.is_empty() {
        if let Some(p) = first_pos_pattern {
            for part in p.split('\n') {
                if !part.is_empty() {
                    pattern_tokens.push(PatternToken::Atom(part.to_string()));
                }
            }
        }
    }

    if pattern_tokens.is_empty() {
        bail!("no pattern given");
    }

    if args.open_in_pager && args.cached {
        bail!("--open-files-in-pager only works on the worktree");
    }
    if args.open_in_pager && tree_ish.is_some() {
        bail!("--open-files-in-pager only works on the worktree");
    }

    let (expr, atom_strings) = crate::commands::grep_expr::parse_pattern_tokens(&pattern_tokens)?;
    let compiled = build_compiled_grep(expr, &atom_strings, &args)?;
    if args.invert_match {
        args.only_matching = false;
    }

    for pat in &atom_strings {
        if !args.perl_regexp && pat.as_bytes().contains(&0) {
            bail!(
                "fatal: given pattern contains NULL byte (via -f <file>). This is only supported with -P under PCRE v2"
            );
        }
    }

    let mut open_paths: Option<Vec<String>> = if args.open_in_pager {
        Some(Vec::new())
    } else {
        None
    };

    if open_paths.is_some() {
        args.color = "never".to_string();
        args.files_with_matches = true;
    }

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

    let found_any;
    if args.no_index {
        // --no-index: walk the filesystem instead of using the index
        let start_dir = std::env::current_dir().context("cannot get current directory")?;
        found_any = grep_filesystem(
            &start_dir,
            "",
            &compiled,
            &args,
            &pathspecs,
            &mut need_sep,
            out,
            &mut open_paths,
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
            &compiled,
            &args,
            &pathspecs,
            Some(tree_spec),
            &mut need_sep,
            out,
            &diff_attrs,
            &mut open_paths,
        )?;
    } else if args.cached {
        // Search index blobs (--cached)
        found_any = grep_cached(
            &repo,
            "",
            &compiled,
            &args,
            &pathspecs,
            &mut need_sep,
            out,
            &mut open_paths,
        )?;
    } else {
        // Search working tree (tracked files from index)
        found_any = grep_worktree(
            &repo,
            "",
            &compiled,
            &args,
            &pathspecs,
            &mut need_sep,
            out,
            &mut open_paths,
        )?;
    }

    if found_any {
        if let Some(paths) = open_paths {
            let pager_cmd = match &args.open_pager_cmd {
                Some(s) if !s.is_empty() => s.clone(),
                _ => resolve_git_pager(&config),
            };
            let work_dir = std::env::current_dir().context("cannot get current directory")?;
            run_open_in_pager(
                &work_dir,
                &pager_cmd,
                &atom_strings,
                args.ignore_case,
                &paths,
            )?;
        }
        Ok(())
    } else {
        std::process::exit(1);
    }
}

/// Resolve pager like `git var GIT_PAGER`: env, then `core.pager`, then `PAGER`, then `cat`.
fn resolve_git_pager(config: &Option<ConfigSet>) -> String {
    std::env::var("GIT_PAGER")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            config
                .as_ref()
                .and_then(|c| c.get("core.pager"))
                .filter(|s| !s.is_empty())
        })
        .or_else(|| std::env::var("PAGER").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "cat".to_owned())
}

/// True if the pager command must be run via `sh -c` (Git's `prepare_shell_cmd` heuristic).
fn pager_needs_shell(pager_cmd: &str) -> bool {
    const META: &[u8] = b"|&;<>()$`\\\"' \t\n*?[#~=%";
    pager_cmd.as_bytes().iter().any(|b| META.contains(b))
}

/// Strip leading directory from pager argv0 when Git would (path ending in `/xxxx`).
fn pager_executable_for_argv0(pager_cmd: &str) -> String {
    let b = pager_cmd.as_bytes();
    let len = b.len();
    if len > 4 && (b[len - 5] == b'/' || b[len - 5] == b'\\') {
        pager_cmd[len - 4..].to_owned()
    } else {
        pager_cmd.to_owned()
    }
}

/// Run the pager with collected file paths, matching Git's `run_pager` + `prepare_shell_cmd`.
fn run_open_in_pager(
    work_dir: &Path,
    pager_cmd: &str,
    patterns: &[String],
    ignore_case: bool,
    files: &[String],
) -> Result<()> {
    let exec0 = pager_executable_for_argv0(pager_cmd);
    let base = Path::new(&exec0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&exec0);

    let mut extra: Vec<String> = Vec::new();
    if patterns.len() == 1 {
        let pat = &patterns[0];
        if base == "less" || base == "vi" {
            if ignore_case && base == "less" {
                extra.push("-I".to_string());
            }
            let star = if base == "less" { "*" } else { "" };
            extra.push(format!("+/{star}{pat}"));
        }
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let status = if pager_needs_shell(pager_cmd) {
        let script = if files.is_empty() {
            pager_cmd.to_string()
        } else {
            format!("{pager_cmd} \"$@\"")
        };
        Command::new(&shell)
            .current_dir(work_dir)
            .arg("-c")
            .arg(&script)
            .args(extra.iter())
            .args(files.iter())
            .status()
    } else {
        Command::new(pager_cmd)
            .current_dir(work_dir)
            .args(extra.iter())
            .args(files.iter())
            .status()
    }
    .context("failed to run pager for --open-files-in-pager")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Grep the index (--cached mode), optionally recursing into submodules.
/// `path_prefix` is prepended to filenames for submodule display (e.g. "submodule/").
fn grep_cached(
    repo: &Repository,
    path_prefix: &str,
    compiled: &CompiledGrep,
    args: &Args,
    pathspecs: &[String],
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    open_paths: &mut Option<Vec<String>>,
) -> Result<bool> {
    let index = repo.load_index().context("loading index")?;
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
    let mut seen_stage0_paths = std::collections::HashSet::new();
    let mut found_any = false;

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        let display_path = format!("{path_prefix}{path_str}");

        // Pathspec filtering is relative to the superproject
        let is_submodule = entry.mode == MODE_GITLINK;
        if !pathspecs.is_empty() && !any_pathspec_matches(&display_path, pathspecs, is_submodule) {
            continue;
        }

        // Submodule entry (gitlink)
        if is_submodule {
            if entry.stage() != 0 {
                continue;
            }
            if !seen_stage0_paths.insert(path_str.clone()) {
                continue;
            }
            if args.recurse_submodules {
                if let Some(work_tree) = &repo.work_tree {
                    let sub_path = work_tree.join(&path_str);
                    if let Ok(sub_repo) = open_submodule_repo(&sub_path) {
                        if grep_cached(
                            &sub_repo,
                            &format!("{display_path}/"),
                            compiled,
                            args,
                            pathspecs,
                            need_sep,
                            out,
                            open_paths,
                        )? {
                            found_any = true;
                        }
                    }
                }
            }
            continue;
        }

        if let Some(max_depth) = args.effective_max_depth() {
            if !path_allowed_at_max_depth(&display_path, pathspecs, max_depth) {
                continue;
            }
        }

        if entry.stage() != 0 && entry.mode != MODE_GITLINK && entry.mode != MODE_TREE {
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
                    let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                    if grep_content(
                        &rel, None, &content, compiled, args, need_sep, out, open_paths,
                    )? {
                        found_any = true;
                    }
                } else {
                    let content = String::from_utf8_lossy(&obj.data);
                    let has_match = compiled
                        .atoms
                        .iter()
                        .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content)));
                    if has_match {
                        writeln!(out, "Binary file {} matches", display_path)?;
                        found_any = true;
                    }
                }
            } else {
                let content = String::from_utf8_lossy(&obj.data);
                let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                if grep_content(
                    &rel, None, &content, compiled, args, need_sep, out, open_paths,
                )? {
                    found_any = true;
                }
            }
            continue;
        }

        if entry.stage() != 0 {
            continue;
        }
        if !seen_stage0_paths.insert(path_str.clone()) {
            continue;
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
                let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                if grep_content(
                    &rel, None, &content, compiled, args, need_sep, out, open_paths,
                )? {
                    found_any = true;
                }
            } else {
                let content = String::from_utf8_lossy(&obj.data);
                let has_match = compiled
                    .atoms
                    .iter()
                    .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content)));
                if has_match {
                    writeln!(out, "Binary file {} matches", display_path)?;
                    found_any = true;
                }
            }
        } else {
            let content = String::from_utf8_lossy(&obj.data);
            let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
            if grep_content(
                &rel, None, &content, compiled, args, need_sep, out, open_paths,
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
    compiled: &CompiledGrep,
    args: &Args,
    pathspecs: &[String],
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    open_paths: &mut Option<Vec<String>>,
) -> Result<bool> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot grep in bare repository"))?;

    let index = repo.load_index().context("loading index")?;
    let diff_attrs = load_diff_attrs(work_tree);
    let mut seen_stage0 = std::collections::HashSet::new();
    let mut unmerged_worktree_grepped = std::collections::HashSet::new();
    let mut found_any = false;

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        let display_path = worktree_display_rel(repo, path_prefix, &path_str, args);

        // Pathspec filtering is relative to the superproject
        let is_submodule = entry.mode == MODE_GITLINK;
        if !pathspecs.is_empty() && !any_pathspec_matches(&display_path, pathspecs, is_submodule) {
            continue;
        }

        // Submodule entry (gitlink)
        if is_submodule {
            if entry.stage() != 0 {
                continue;
            }
            if !seen_stage0.insert(path_str.clone()) {
                continue;
            }
            if args.recurse_submodules {
                let sub_path = work_tree.join(&path_str);
                let sub_present = sub_path.join(".git").try_exists().unwrap_or(false);
                if entry.skip_worktree() && !sub_present {
                    continue;
                }
                if let Ok(sub_repo) = open_submodule_repo(&sub_path) {
                    if grep_worktree(
                        &sub_repo,
                        &format!("{display_path}/"),
                        compiled,
                        args,
                        pathspecs,
                        need_sep,
                        out,
                        open_paths,
                    )? {
                        found_any = true;
                    }
                }
            }
            continue;
        }

        // Apply max-depth filter
        if let Some(max_depth) = args.effective_max_depth() {
            if !path_allowed_at_max_depth(&display_path, pathspecs, max_depth) {
                continue;
            }
        }

        let full_path = work_tree.join(&path_str);

        // Merge conflicts: grep the work tree file once (matches git grep).
        if entry.stage() != 0 && entry.mode != MODE_GITLINK && entry.mode != MODE_TREE {
            if !unmerged_worktree_grepped.insert(path_str.clone()) {
                continue;
            }
            let content = match std::fs::read(&full_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let content_is_binary = content.iter().take(8000).any(|&b| b == 0);
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
                    let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                    if grep_content(
                        &rel,
                        None,
                        &content_str,
                        compiled,
                        args,
                        need_sep,
                        out,
                        open_paths,
                    )? {
                        found_any = true;
                    }
                } else {
                    let content_str = String::from_utf8_lossy(&content);
                    let has_match = compiled
                        .atoms
                        .iter()
                        .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content_str)));
                    if has_match {
                        writeln!(out, "Binary file {} matches", display_path)?;
                        found_any = true;
                    }
                }
            } else {
                let content_str = String::from_utf8_lossy(&content);
                let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                if grep_content(
                    &rel,
                    None,
                    &content_str,
                    compiled,
                    args,
                    need_sep,
                    out,
                    open_paths,
                )? {
                    found_any = true;
                }
            }
            continue;
        }

        if entry.stage() != 0 {
            continue;
        }
        if !seen_stage0.insert(path_str.clone()) {
            continue;
        }

        // CE_VALID alone uses the index blob. With SKIP_WORKTREE, git never takes the
        // cached-only path: absent paths are skipped; present paths are read from disk
        // (t7817 sparse + manual file, and assume-unchanged + sparse without a file).
        let in_index = entry.assume_unchanged() && !entry.skip_worktree();
        if in_index {
            if entry.intent_to_add() {
                continue;
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
                    let content_str = String::from_utf8_lossy(&obj.data);
                    let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                    if grep_content(
                        &rel,
                        None,
                        &content_str,
                        compiled,
                        args,
                        need_sep,
                        out,
                        open_paths,
                    )? {
                        found_any = true;
                    }
                } else {
                    let content_str = String::from_utf8_lossy(&obj.data);
                    let has_match = compiled
                        .atoms
                        .iter()
                        .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content_str)));
                    if has_match {
                        writeln!(out, "Binary file {} matches", display_path)?;
                        found_any = true;
                    }
                }
            } else {
                let content_str = String::from_utf8_lossy(&obj.data);
                let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                if grep_content(
                    &rel,
                    None,
                    &content_str,
                    compiled,
                    args,
                    need_sep,
                    out,
                    open_paths,
                )? {
                    found_any = true;
                }
            }
            continue;
        }

        if entry.skip_worktree() && !full_path.exists() && !full_path.is_symlink() {
            continue;
        }

        let content = match std::fs::read(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let content_is_binary = content.iter().take(8000).any(|&b| b == 0);
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
                let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
                if grep_content(
                    &rel,
                    None,
                    &content_str,
                    compiled,
                    args,
                    need_sep,
                    out,
                    open_paths,
                )? {
                    found_any = true;
                }
            } else {
                let content_str = String::from_utf8_lossy(&content);
                let has_match = compiled
                    .atoms
                    .iter()
                    .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content_str)));
                if has_match {
                    writeln!(out, "Binary file {} matches", display_path)?;
                    found_any = true;
                }
            }
        } else {
            let content_str = String::from_utf8_lossy(&content);
            let rel = worktree_display_rel(repo, path_prefix, &path_str, args);
            if grep_content(
                &rel,
                None,
                &content_str,
                compiled,
                args,
                need_sep,
                out,
                open_paths,
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
        if pathspec == "." {
            return true;
        }
        path == pathspec
            || path.starts_with(&format!("{pathspec}/"))
            || (is_dir && pathspec.starts_with(&format!("{path}/")))
    }
}

/// Check if any pathspec matches a path.
fn any_pathspec_matches(path: &str, pathspecs: &[String], is_dir: bool) -> bool {
    pathspecs.iter().any(|p| matches_pathspec(path, p, is_dir))
}

/// Collapse `.`, `..`, and empty segments in a `/`-separated repo-relative path.
fn normalize_repo_rel_path(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            stack.pop();
        } else {
            stack.push(part);
        }
    }
    if stack.is_empty() {
        ".".to_string()
    } else {
        stack.join("/")
    }
}

/// Rebase pathspecs from the current working directory (Git pathspec behavior).
fn pathspecs_relative_to_cwd(repo: &Repository, pathspecs: &[String]) -> Vec<String> {
    let Some(wt) = repo.work_tree.as_deref() else {
        return pathspecs.to_vec();
    };
    let Ok(cwd) = std::env::current_dir() else {
        return pathspecs.to_vec();
    };
    let Ok(rel) = cwd.strip_prefix(wt) else {
        return pathspecs.to_vec();
    };
    let prefix = rel.to_string_lossy();
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        return pathspecs.to_vec();
    }
    pathspecs
        .iter()
        .map(|s| {
            if Path::new(s).is_absolute() {
                s.clone()
            } else {
                normalize_repo_rel_path(&format!("{prefix}/{s}"))
            }
        })
        .collect()
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
    if let Ok(index) = repo.load_index() {
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
    compiled: &CompiledGrep,
    args: &Args,
    pathspecs: &[String],
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    open_paths: &mut Option<Vec<String>>,
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
                compiled,
                args,
                pathspecs,
                need_sep,
                out,
                open_paths,
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
                let has_match = compiled
                    .atoms
                    .iter()
                    .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content_str)));
                if has_match {
                    writeln!(out, "Binary file {} matches", display_path)?;
                    found_any = true;
                }
            } else {
                let content_str = String::from_utf8_lossy(&content);
                if grep_content(
                    &display_path,
                    None,
                    &content_str,
                    compiled,
                    args,
                    need_sep,
                    out,
                    open_paths,
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

/// Parse positional arguments into (optional first pattern, tree_ish, pathspecs).
fn parse_positional(
    args: &Args,
    repo: &Repository,
    has_peeled_patterns: bool,
) -> Result<(Option<String>, Option<String>, Vec<String>)> {
    let positional = &args.positional;

    let sep_pos = positional.iter().position(|a| a == "--");

    let (before_sep, pathspecs) = match sep_pos {
        Some(pos) => (&positional[..pos], positional[pos + 1..].to_vec()),
        None => (positional.as_slice(), Vec::new()),
    };

    let mut tree_ish = None;

    if !has_peeled_patterns {
        if before_sep.is_empty() {
            return Ok((None, tree_ish, pathspecs));
        }
        let first = before_sep[0].clone();
        let rest = &before_sep[1..];
        if !rest.is_empty() && is_revision(repo, &rest[0]) {
            tree_ish = Some(rest[0].clone());
            let mut ps = pathspecs;
            ps.extend(rest[1..].iter().cloned());
            return Ok((Some(first), tree_ish, ps));
        }
        let mut ps = pathspecs;
        ps.extend(rest.iter().cloned());
        return Ok((Some(first), tree_ish, ps));
    }

    if !before_sep.is_empty() && is_revision(repo, &before_sep[0]) {
        tree_ish = Some(before_sep[0].clone());
        let mut ps = pathspecs;
        ps.extend(before_sep[1..].iter().cloned());
        return Ok((None, tree_ish, ps));
    }

    let mut ps = pathspecs;
    ps.extend(before_sep.iter().cloned());
    Ok((None, tree_ish, ps))
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

fn build_one_regex(pat: &str, args: &Args) -> Result<Regex> {
    let use_bre = !args.extended_regexp && !args.fixed_strings && !args.perl_regexp;
    // Git ERE does not accept PCRE `\p{...}` / `\P{...}`; Rust's engine does — reject for parity.
    if args.extended_regexp && !args.perl_regexp && !args.fixed_strings {
        if pat.contains("\\p{") || pat.contains("\\P{") {
            bail!("invalid pattern: '{pat}'");
        }
    }
    let effective = if args.fixed_strings {
        regex::escape(pat)
    } else if use_bre {
        bre_to_ere(pat)
    } else if args.perl_regexp {
        pat.to_string()
    } else {
        fix_charclass_escapes(pat)
    };
    let effective = if args.word_regexp {
        format!(r"\b{effective}\b")
    } else {
        effective
    };
    RegexBuilder::new(&effective)
        .case_insensitive(args.ignore_case)
        .build()
        .with_context(|| format!("invalid pattern: '{pat}'"))
}

fn build_compiled_grep(
    expr: GrepExpr,
    atom_strings: &[String],
    args: &Args,
) -> Result<CompiledGrep> {
    if args.perl_regexp {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: cannot use Perl-compatible regexes when not compiled with USE_LIBPCRE"
                .to_string(),
        }));
    }
    let mut atoms = Vec::with_capacity(atom_strings.len());
    for pat in atom_strings {
        atoms.push(Some(build_one_regex(pat, args)?));
    }
    Ok(CompiledGrep { atoms, expr })
}

fn word_char_git(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Git-compatible next match for `--only-matching` with `-w` (retry like `headerless_match_one_pattern`).
fn next_match_from_bol(
    line: &str,
    bol: usize,
    args: &Args,
    compiled: &CompiledGrep,
    atom_indices: &[usize],
) -> Option<(usize, usize)> {
    let end = line.len();
    if bol > end {
        return None;
    }
    let mut best: Option<(usize, usize)> = None;
    for &ai in atom_indices {
        let Some(re) = compiled.atoms.get(ai).and_then(|x| x.as_ref()) else {
            continue;
        };
        let m = if args.word_regexp {
            next_word_bounded_match(line, re, bol)
        } else {
            line.get(bol..)
                .and_then(|sl| re.find(sl))
                .map(|m| (bol + m.start(), bol + m.end()))
        };
        let Some((abs_s, abs_e)) = m else {
            continue;
        };
        if abs_s > end || abs_e > end {
            continue;
        }
        let take = match best {
            None => true,
            Some((bs, _be)) if abs_s < bs => true,
            Some((bs, be)) if abs_s == bs && abs_e > be => true,
            _ => false,
        };
        if take {
            best = Some((abs_s, abs_e));
        }
    }
    best
}

fn next_word_bounded_match(line: &str, re: &Regex, mut search: usize) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let end = line.len();
    while search < end {
        let slice = line.get(search..)?;
        let m = re.find(slice)?;
        let abs_s = search + m.start();
        let abs_e = search + m.end();
        let left_ok = abs_s == 0 || !word_char_git(bytes[abs_s - 1]);
        let right_ok = abs_e >= end || !word_char_git(bytes[abs_e]);
        if left_ok && right_ok && abs_s < abs_e {
            return Some((abs_s, abs_e));
        }
        let mut bol = abs_s + 1;
        while bol < end && word_char_git(bytes[bol - 1]) && word_char_git(bytes[bol]) {
            bol += 1;
        }
        search = bol;
    }
    None
}

fn cwd_strip_repo_rel(repo: &Repository, repo_rel_path: &str, args: &Args) -> String {
    if args.full_name {
        return repo_rel_path.to_string();
    }
    let Some(wt) = repo.work_tree.as_deref() else {
        return repo_rel_path.to_string();
    };
    let Ok(cwd) = std::env::current_dir() else {
        return repo_rel_path.to_string();
    };
    let Ok(rel) = cwd.strip_prefix(wt) else {
        return repo_rel_path.to_string();
    };
    let rel = rel.to_string_lossy();
    let rel = rel.trim_start_matches('/').trim_end_matches('/');
    if rel.is_empty() {
        return repo_rel_path.to_string();
    }
    let prefix = format!("{rel}/");
    if repo_rel_path.starts_with(&prefix) {
        repo_rel_path[prefix.len()..].to_string()
    } else {
        repo_rel_path.to_string()
    }
}

fn worktree_display_rel(
    repo: &Repository,
    path_prefix: &str,
    path_str: &str,
    args: &Args,
) -> String {
    let full = format!("{path_prefix}{path_str}");
    cwd_strip_repo_rel(repo, &full, args)
}

fn path_for_output(path: &str, args: &Args) -> String {
    if args.null_following_name {
        path.to_string()
    } else {
        quote_path_for_check_attr(path)
    }
}

/// Git `within_depth`: count `/` in `tail`; each slash increments depth; require depth <= max_depth.
fn within_depth_tail(tail: &str, max_depth: usize) -> bool {
    let mut depth = 0usize;
    for b in tail.as_bytes() {
        if *b == b'/' {
            depth += 1;
            if depth > max_depth {
                return false;
            }
        }
    }
    depth <= max_depth
}

fn path_allowed_at_max_depth(path: &str, pathspecs: &[String], max_depth: usize) -> bool {
    if pathspecs.is_empty() {
        return path.matches('/').count() <= max_depth;
    }
    // Git ignores --max-depth when pathspecs contain active wildcards (see git-grep(1)).
    if pathspecs.iter().any(|p| has_glob_chars(p)) {
        return true;
    }
    pathspecs.iter().any(|ps| {
        if ps == "." {
            return within_depth_tail(path, max_depth);
        }
        if path == ps.as_str() {
            return within_depth_tail("", max_depth);
        }
        let prefix = format!("{ps}/");
        if path.starts_with(&prefix) {
            return within_depth_tail(&path[prefix.len()..], max_depth);
        }
        false
    })
}

// Color constants
const COLOR_FILENAME: &str = "\x1b[35m";
const COLOR_LINENO: &str = "\x1b[32m";
const COLOR_COLUMNNO: &str = "\x1b[32m";
const COLOR_MATCH: &str = "\x1b[1;31m";
const COLOR_SEP: &str = "\x1b[36m";
const COLOR_RESET: &str = "\x1b[m";

fn sep_char(ch: char, color: bool) -> String {
    if color {
        format!("{COLOR_SEP}{ch}{COLOR_RESET}")
    } else {
        ch.to_string()
    }
}

fn sep_out(args: &Args, color: bool, ch: char) -> String {
    if args.null_following_name {
        "\0".to_string()
    } else {
        sep_char(ch, color)
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

fn content_satisfies_all_match(compiled: &CompiledGrep, content: &str) -> bool {
    for re_opt in &compiled.atoms {
        let Some(re) = re_opt.as_ref() else {
            continue;
        };
        if !content.lines().any(|line| re.is_match(line)) {
            return false;
        }
    }
    true
}

/// 1-based column of first match (Git semantics) for a line.
fn column_for_line(line: &str, compiled: &CompiledGrep, invert: bool) -> usize {
    let mut col: isize = -1;
    let mut icol: isize = -1;
    let hit = match_expr_eval(
        &compiled.expr,
        line,
        &compiled.atoms,
        &mut col,
        &mut icol,
        true,
    );
    let _ = hit;
    let cno = if invert { icol } else { col };
    if cno < 0 {
        1
    } else {
        (cno + 1) as usize
    }
}

fn colorize_line(line: &str, compiled: &CompiledGrep, atom_indices: &[usize]) -> String {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &i in atom_indices {
        if let Some(re) = compiled.atoms.get(i).and_then(|x| x.as_ref()) {
            for m in re.find_iter(line) {
                ranges.push((m.start(), m.end()));
            }
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

fn line_prefix(
    display_name: &str,
    sep: char,
    args: &Args,
    color: bool,
    lno: Option<usize>,
    col: Option<usize>,
) -> String {
    let mut s = String::new();
    if !display_name.is_empty() {
        s.push_str(&fmt_name(display_name, color));
        s.push_str(&sep_out(args, color, sep));
    }
    if let Some(n) = lno {
        s.push_str(&fmt_num(n, color));
        s.push_str(&sep_out(args, color, sep));
    }
    if let Some(c) = col {
        s.push_str(&fmt_col(c, color));
        s.push_str(&sep_out(args, color, sep));
    }
    s
}

/// Search content of a single file. Returns true if any match found.
/// `relative_path` is repo-relative (may be cwd-stripped for work tree / tree search).
/// `rev_label` is e.g. `Some("HEAD")` for object-store grep (`HEAD:path` in output).
fn grep_content(
    relative_path: &str,
    rev_label: Option<&str>,
    content: &str,
    compiled: &CompiledGrep,
    args: &Args,
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    open_paths: &mut Option<Vec<String>>,
) -> Result<bool> {
    let color = args.use_color();
    let quoted_path = path_for_output(relative_path, args);
    let full_display = match rev_label {
        Some(r) => format!("{r}:{quoted_path}"),
        None => quoted_path,
    };
    let show_name = args.show_filename();
    let display_name = if show_name {
        full_display.clone()
    } else {
        String::new()
    };

    let mut atom_indices_all: Vec<usize> = Vec::new();
    collect_atom_indices(&compiled.expr, &mut atom_indices_all);

    let lines: Vec<&str> = content.lines().collect();
    let nlines = lines.len();
    let before = args.before_ctx();
    let after = args.after_ctx();
    let use_context = args.has_context();
    let col_mode = args.column;

    let mut match_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut hit = line_matches_expr(&compiled.expr, line, &compiled.atoms, col_mode);
        if args.invert_match {
            hit = !hit;
        }
        if hit {
            match_indices.push(i);
        }
    }

    if args.all_match && !content_satisfies_all_match(compiled, content) {
        match_indices.clear();
    }

    if let Some(max) = args.max_count {
        if max >= 0 {
            match_indices.truncate(max as usize);
        }
    }

    let has_match = !match_indices.is_empty();
    let match_count = match_indices.len() as u64;

    let display_name = if args.heading && has_match && show_name {
        String::new()
    } else {
        display_name
    };

    if args.files_with_matches {
        if has_match {
            if let Some(paths) = open_paths {
                paths.push(relative_path.to_string());
            } else {
                writeln!(out, "{}", fmt_name(&display_name, color))?;
            }
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
            let prefix = line_prefix(&display_name, ':', args, color, None, None);
            write!(out, "{prefix}{match_count}")?;
            writeln!(out)?;
        }
        return Ok(has_match);
    }

    if !has_match {
        return Ok(false);
    }

    if args.file_break && *need_sep {
        writeln!(out)?;
    }

    if args.heading && show_name {
        writeln!(out, "{}", fmt_name(&full_display, color))?;
    }

    if args.only_matching {
        for &idx in &match_indices {
            let line = lines[idx];
            let mut bol = 0usize;
            let mut cno = 0usize;
            let mut first = true;
            loop {
                let Some((abs_s, abs_e)) =
                    next_match_from_bol(line, bol, args, compiled, &atom_indices_all)
                else {
                    break;
                };
                if first {
                    cno = abs_s + 1;
                    first = false;
                }
                let matched_text = line.get(abs_s..abs_e).unwrap_or("");
                let prefix = if args.column {
                    line_prefix(&display_name, ':', args, color, Some(idx + 1), Some(cno))
                } else {
                    line_prefix(&display_name, ':', args, color, Some(idx + 1), None)
                };
                if color {
                    writeln!(out, "{prefix}{COLOR_MATCH}{matched_text}{COLOR_RESET}")?;
                } else {
                    writeln!(out, "{prefix}{matched_text}")?;
                }
                cno += abs_e - bol;
                bol = abs_e;
            }
        }
        *need_sep = true;
        return Ok(true);
    }

    if use_context {
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for &idx in &match_indices {
            let start = idx.saturating_sub(before);
            let end = if nlines == 0 {
                0
            } else {
                (idx + after).min(nlines - 1)
            };
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
            if *need_sep {
                writeln!(out, "--")?;
            }
            *need_sep = true;

            for i in start..=end {
                let is_match_line = match_set.contains(&i);
                let sep = if is_match_line { ':' } else { '-' };
                let col = if args.column && is_match_line {
                    Some(column_for_line(lines[i], compiled, args.invert_match))
                } else {
                    None
                };
                let prefix = line_prefix(
                    &display_name,
                    sep,
                    args,
                    color,
                    args.show_line_number().then_some(i + 1),
                    col,
                );
                if color && is_match_line {
                    writeln!(
                        out,
                        "{}{}",
                        prefix,
                        colorize_line(lines[i], compiled, &atom_indices_all)
                    )?;
                } else {
                    writeln!(out, "{}{}", prefix, lines[i])?;
                }
            }
        }
    } else {
        for &idx in &match_indices {
            let line = lines[idx];
            let col = args
                .column
                .then_some(column_for_line(line, compiled, args.invert_match));
            let prefix = line_prefix(
                &display_name,
                ':',
                args,
                color,
                args.show_line_number().then_some(idx + 1),
                col,
            );
            if color {
                writeln!(
                    out,
                    "{}{}",
                    prefix,
                    colorize_line(line, compiled, &atom_indices_all)
                )?;
            } else {
                writeln!(out, "{prefix}{line}")?;
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
    _depth: usize,
    compiled: &CompiledGrep,
    args: &Args,
    pathspecs: &[String],
    tree_name: Option<&str>,
    need_sep: &mut bool,
    out: &mut (impl Write + ?Sized),
    diff_attrs: &[DiffAttrRule],
    open_paths: &mut Option<Vec<String>>,
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
            && !any_pathspec_matches(&full_name, pathspecs, is_tree || is_gitlink)
        {
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
                            compiled,
                            args,
                            pathspecs,
                            tree_name,
                            need_sep,
                            out,
                            &sub_diff_attrs,
                            open_paths,
                        )? {
                            found = true;
                        }
                    }
                }
            }
            continue;
        }

        if is_tree {
            let sub_obj = repo.odb.read(&entry.oid)?;
            if grep_tree(
                repo,
                &sub_obj.data,
                &full_name,
                _depth + 1,
                compiled,
                args,
                pathspecs,
                tree_name,
                need_sep,
                out,
                diff_attrs,
                open_paths,
            )? {
                found = true;
            }
        } else {
            if let Some(max_depth) = args.effective_max_depth() {
                if !path_allowed_at_max_depth(&full_name, pathspecs, max_depth) {
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
                let has_match = compiled
                    .atoms
                    .iter()
                    .any(|r| r.as_ref().is_some_and(|re| re.is_match(&content)));
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
                let rel = cwd_strip_repo_rel(repo, &full_name, args);
                if grep_content(
                    &rel, tree_name, &content, compiled, args, need_sep, out, open_paths,
                )? {
                    found = true;
                }
            }
        }
    }

    Ok(found)
}
