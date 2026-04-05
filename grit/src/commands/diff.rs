//! `grit diff` — show changes between commits, commit and working tree, etc.
//!
//! Modes:
//! - No revisions: working tree vs index (unstaged changes)
//! - `--cached [<commit>]`: index vs HEAD (or specified commit) — staged changes
//! - `<commit>`: commit's tree vs working tree (combined view)
//! - `<commit> <commit>`: commit-to-commit diff
//!
//! Output formats: unified patch (default), `--stat`, `--shortstat`,
//! `--numstat`, `--name-only`, `--name-status`.
//!
//! Exit codes: `--exit-code` / `--quiet` return exit code 1 if there are
//! differences.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{
    anchored_unified_diff, count_changes, detect_renames, diff_index_to_tree,
    diff_index_to_worktree, diff_tree_to_worktree, diff_trees, unified_diff, zero_oid, DiffEntry,
    DiffStatus,
};
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use unicode_width::UnicodeWidthStr;

/// ANSI color codes for diff output.
const RESET: &str = "\x1b[m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";

/// Whitespace-ignore options bundled together.
#[derive(Debug, Default)]
struct WhitespaceMode {
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_space_at_eol: bool,
    ignore_blank_lines: bool,
    ignore_cr_at_eol: bool,
}

impl WhitespaceMode {
    /// Returns true if any whitespace-ignore option is active.
    fn any(&self) -> bool {
        self.ignore_all_space
            || self.ignore_space_change
            || self.ignore_space_at_eol
            || self.ignore_blank_lines
            || self.ignore_cr_at_eol
    }

    /// Normalize a string according to the active whitespace modes.
    fn normalize(&self, s: &str) -> String {
        let mut lines: Vec<String> = s.lines().map(|l| self.normalize_line(l)).collect();
        if self.ignore_blank_lines {
            lines.retain(|l| !l.trim().is_empty());
        }
        lines.join("\n")
    }

    /// Normalize a single line according to the active whitespace modes.
    fn normalize_line(&self, line: &str) -> String {
        let mut s = line.to_owned();

        // --ignore-cr-at-eol: strip trailing CR
        if self.ignore_cr_at_eol
            && s.ends_with('\r') {
                s.truncate(s.len() - 1);
            }

        // -w / --ignore-all-space: remove all whitespace
        if self.ignore_all_space {
            s = s.chars().filter(|c| !c.is_whitespace()).collect();
            return s;
        }

        // -b / --ignore-space-change: collapse runs of whitespace to single space
        if self.ignore_space_change {
            let mut result = String::with_capacity(s.len());
            let mut in_space = false;
            for c in s.chars() {
                if c.is_whitespace() {
                    if !in_space {
                        result.push(' ');
                        in_space = true;
                    }
                } else {
                    result.push(c);
                    in_space = false;
                }
            }
            s = result.trim_end().to_owned();
            return s;
        }

        // --ignore-space-at-eol: strip trailing whitespace
        if self.ignore_space_at_eol {
            s = s.trim_end().to_owned();
        }

        s
    }
}

/// Arguments for `grit diff`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show changes between commits, commit and working tree, etc.")]
pub struct Args {
    /// Show staged changes (index vs HEAD). Alias: --staged.
    #[arg(long = "cached", alias = "staged")]
    pub cached: bool,

    /// Show a diffstat summary instead of the patch.
    /// Accepts optional `--stat=<width>[,<name-width>[,<count>]]`.
    #[arg(long = "stat", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    pub stat: Option<String>,

    /// Limit the number of files shown in --stat output.
    #[arg(long = "stat-count")]
    pub stat_count: Option<usize>,

    /// Set the width of the --stat output.
    #[arg(long = "stat-width")]
    pub stat_width: Option<usize>,

    /// Set the width of the graph portion of --stat output.
    #[arg(long = "stat-graph-width")]
    pub stat_graph_width: Option<usize>,

    /// Set the width of the filename portion of --stat output.
    #[arg(long = "stat-name-width")]
    pub stat_name_width: Option<usize>,

    /// Show only the summary line: N files changed, N insertions(+), N deletions(-).
    #[arg(long = "shortstat")]
    pub shortstat: bool,

    /// Show machine-readable stat (additions/deletions per file).
    #[arg(long = "numstat")]
    pub numstat: bool,

    /// Show only the names of changed files.
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Show the names and status of changed files.
    #[arg(long = "name-status")]
    pub name_status: bool,

    /// Show raw diff format (:old-mode new-mode old-oid new-oid status\tpath).
    #[arg(long = "raw")]
    pub raw: bool,

    /// Do not abbreviate object IDs.
    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    /// Show the full object ID in raw and index output.
    #[arg(long = "full-index")]
    pub full_index: bool,

    /// Abbreviation length for object IDs in raw output.
    #[arg(long = "abbrev", value_name = "N")]
    pub abbrev: Option<usize>,

    /// Merge hunks that are within <N> lines of each other.
    #[arg(long = "inter-hunk-context", value_name = "N")]
    pub inter_hunk_context: Option<usize>,

    /// Ignore submodule changes. Accepts: all, dirty, untracked, none.
    #[arg(long = "ignore-submodules", value_name = "WHEN", default_missing_value = "all", num_args = 0..=1, require_equals = true)]
    pub ignore_submodules: Option<String>,

    /// Detect and color moved lines differently.
    #[arg(long = "color-moved", value_name = "MODE", default_missing_value = "default", num_args = 0..=1, require_equals = true)]
    pub color_moved: Option<String>,

    /// Break complete rewrite into delete + add pair.
    #[arg(short = 'B', long = "break-rewrites")]
    pub break_rewrites: bool,

    /// Output a binary diff that can be applied with git-apply.
    #[arg(long = "binary")]
    pub binary: bool,

    /// Show a condensed summary of extended header info (renames, mode changes).
    #[arg(long = "summary")]
    pub summary: bool,

    /// Exit with status 1 if there are differences, 0 otherwise.
    #[arg(long = "exit-code")]
    pub exit_code: bool,

    /// Suppress diff output; implies --exit-code.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Rotate the output to start at the named file.
    #[arg(long = "rotate-to")]
    pub rotate_to: Option<String>,

    /// Skip output until the named file.
    #[arg(long = "skip-to")]
    pub skip_to: Option<String>,

    /// Order files according to the given orderfile.
    #[arg(short = 'O')]
    pub order_file: Option<String>,

    /// Use the histogram diff algorithm.
    #[arg(long = "histogram")]
    pub histogram: bool,

    /// Use the patience diff algorithm.
    #[arg(long = "patience")]
    pub patience: bool,

    /// Use the minimal diff algorithm.
    #[arg(long = "minimal")]
    pub minimal: bool,

    /// Select a diff algorithm (myers, minimal, patience, histogram).
    #[arg(long = "diff-algorithm")]
    pub diff_algorithm: Option<String>,

    /// Enable indent heuristic (default in modern git).
    #[arg(long = "indent-heuristic")]
    pub indent_heuristic: bool,

    /// Disable indent heuristic.
    #[arg(long = "no-indent-heuristic")]
    pub no_indent_heuristic: bool,

    /// Anchor the diff on the given text (can be repeated).
    #[arg(long = "anchored")]
    pub anchored: Vec<String>,

    /// Show relative paths from the given subdirectory.
    #[arg(long = "relative", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    pub relative: Option<Option<String>>,

    /// Disable --relative.
    #[arg(long = "no-relative")]
    pub no_relative: bool,

    /// Colorize the output. Values: always, never, auto.
    #[arg(long = "color", value_name = "WHEN", default_missing_value = "always", num_args = 0..=1)]
    pub color: Option<String>,

    /// Show a word-level diff with `[-removed-]{+added+}` markers.
    #[arg(long = "word-diff", value_name = "MODE", default_missing_value = "plain", num_args = 0..=1)]
    pub word_diff: Option<String>,

    /// Show colored word diff (shorthand for --word-diff=color).
    #[arg(long = "color-words")]
    pub color_words: bool,

    /// Ignore all whitespace when comparing lines (-w).
    #[arg(short = 'w', long = "ignore-all-space")]
    pub ignore_all_space: bool,

    /// Ignore changes in amount of whitespace (-b).
    #[arg(short = 'b', long = "ignore-space-change")]
    pub ignore_space_change: bool,

    /// Ignore whitespace at end of line.
    #[arg(long = "ignore-space-at-eol")]
    pub ignore_space_at_eol: bool,

    /// Ignore changes whose lines are all blank.
    #[arg(long = "ignore-blank-lines")]
    pub ignore_blank_lines: bool,

    /// Ignore carriage-return at end of line.
    #[arg(long = "ignore-cr-at-eol")]
    pub ignore_cr_at_eol: bool,

    /// Generate patch output (default behavior; for compatibility with git).
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Number of context lines in unified diff output (default: 3).
    #[arg(
        short = 'U',
        long = "unified",
        value_name = "N",
        allow_hyphen_values = true
    )]
    pub unified: Option<usize>,

    /// Detect renames.
    #[arg(short = 'M', long = "find-renames", value_name = "N", default_missing_value = "50", num_args = 0..=1)]
    pub find_renames: Option<String>,

    /// Suppress diff output for submodules.
    #[arg(long = "submodule", value_name = "FORMAT", default_missing_value = "short", num_args = 0..=1, require_equals = true)]
    pub submodule: Option<String>,

    /// Disable external diff drivers (no-op, for compatibility).
    #[arg(long = "no-ext-diff")]
    pub no_ext_diff: bool,

    /// Compare two paths outside a git repository.
    #[arg(long = "no-index")]
    pub no_index: bool,

    /// Disable external diff drivers (accepted for compatibility, no-op).

    /// Disable textconv filters (accepted for compatibility, no-op).
    #[arg(long = "no-textconv")]
    pub no_textconv: bool,

    /// Check for whitespace errors in the diff.
    #[arg(long = "check")]
    pub check: bool,

    /// Set the source prefix (default: "a/").
    #[arg(long = "src-prefix", value_name = "PREFIX")]
    pub src_prefix: Option<String>,

    /// Set the destination prefix (default: "b/").
    #[arg(long = "dst-prefix", value_name = "PREFIX")]
    pub dst_prefix: Option<String>,

    /// Do not show any source or destination prefix.
    #[arg(long = "no-prefix")]
    pub no_prefix: bool,

    /// Override config and use the default "a/"/"b/" prefix.
    #[arg(long = "default-prefix")]
    pub default_prefix: bool,

    /// Line prefix for every line of output.
    #[arg(long = "line-prefix", value_name = "PREFIX")]
    pub line_prefix: Option<String>,

    /// Disable rename detection (must not be abbreviated).
    #[arg(long = "no-renames")]
    pub no_renames: bool,

    /// Commits or paths. Use `--` to separate revisions from paths.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Run the `diff` command.
pub fn run(mut args: Args) -> Result<()> {
    // Parse --stat=<width>[,<name-width>[,<count>]] into separate fields
    let mut stat_enabled = if let Some(ref val) = args.stat {
        if !val.is_empty() {
            let parts: Vec<&str> = val.split(',').collect();
            if let Some(w) = parts.first().and_then(|s| s.parse::<usize>().ok()) {
                if args.stat_width.is_none() {
                    args.stat_width = Some(w);
                }
            }
            if let Some(nw) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                if args.stat_name_width.is_none() {
                    args.stat_name_width = Some(nw);
                }
            }
            if let Some(c) = parts.get(2).and_then(|s| s.parse::<usize>().ok()) {
                if args.stat_count.is_none() {
                    args.stat_count = Some(c);
                }
            }
        }
        true
    } else {
        false
    };

    // --no-index: compare two files outside a git repository
    if args.no_index {
        return run_no_index(&args);
    }

    let raw_args: Vec<String> = std::env::args().collect();
    let has_separator = raw_args.iter().any(|a| a == "--");
    let (mut revs, paths) = parse_rev_and_paths(&args.args, has_separator);

    let repo = Repository::discover(None).context("not a git repository")?;

    // Resolve diff prefixes from config and command-line options
    let (src_prefix, dst_prefix) = resolve_diff_prefixes(&args, &repo);

    // Expand A...B (symmetric diff) → merge-base(A,B)..B
    // Expand A..B → A B (two-rev diff)
    // trailing_var_arg may capture flags like --name-only into args.
    // Move them back into the flags struct so they take effect.
    let mut extra_revs = Vec::new();
    for r in &revs {
        if r.starts_with("--") || r.starts_with("-") && r.len() > 1 {
            // Re-apply trailing flags
            match r.as_str() {
                "--name-only" => args.name_only = true,
                "--name-status" => args.name_status = true,
                "--numstat" => args.numstat = true,
                "--shortstat" => args.shortstat = true,
                "--summary" => args.summary = true,
                "--quiet" | "-q" => args.quiet = true,
                s if s.starts_with("--stat") => {
                    if s == "--stat" {
                        if args.stat.is_none() {
                            args.stat = Some(String::new());
                        }
                    } else if let Some(val) = s.strip_prefix("--stat=") {
                        args.stat = Some(val.to_owned());
                    }
                    // re-parse stat
                    if let Some(ref val) = args.stat {
                        if !val.is_empty() {
                            let parts: Vec<&str> = val.split(',').collect();
                            if let Some(w) = parts.first().and_then(|s| s.parse::<usize>().ok()) {
                                if args.stat_width.is_none() {
                                    args.stat_width = Some(w);
                                }
                            }
                        }
                    }
                    stat_enabled = true;
                }
                "--exit-code" => args.exit_code = true,
                "--raw" => args.raw = true,
                "--no-abbrev" => args.no_abbrev = true,
                "--full-index" => args.full_index = true,
                "--binary" => args.binary = true,
                s if s.starts_with("--abbrev=") => {
                    if let Some(val) = s.strip_prefix("--abbrev=") {
                        args.abbrev = val.parse().ok();
                    }
                }
                s if s.starts_with("--inter-hunk-context=") => {
                    if let Some(val) = s.strip_prefix("--inter-hunk-context=") {
                        args.inter_hunk_context = val.parse().ok();
                    }
                }
                "--no-ext-diff" => {
                    args.no_ext_diff = true;
                }
                "--no-textconv" => {
                    args.no_textconv = true;
                }
                "--check" => {
                    args.check = true;
                }
                s if s.starts_with("--ignore-submodules") => {
                    args.ignore_submodules = Some("all".to_owned());
                }
                s if s.starts_with("--color-moved") => {
                    args.color_moved = Some("default".to_owned());
                }
                s if s.starts_with("-O") && s.len() > 2 => {
                    args.order_file = Some(s[2..].to_string());
                }
                "--no-prefix" => {
                    args.no_prefix = true;
                }
                "--default-prefix" => {
                    args.default_prefix = true;
                }
                "--no-renames" => {
                    args.no_renames = true;
                }
                s if s.starts_with("--src-prefix=") => {
                    args.src_prefix =
                        Some(s.strip_prefix("--src-prefix=").unwrap_or("").to_owned());
                }
                s if s.starts_with("--dst-prefix=") => {
                    args.dst_prefix =
                        Some(s.strip_prefix("--dst-prefix=").unwrap_or("").to_owned());
                }
                s if s.starts_with("--line-prefix=") => {
                    args.line_prefix =
                        Some(s.strip_prefix("--line-prefix=").unwrap_or("").to_owned());
                }
                _ => {
                    extra_revs.push(r.clone());
                    continue;
                }
            }
        } else {
            extra_revs.push(r.clone());
        }
    }
    revs = extra_revs;

    let mut _symmetric = false;
    if revs.len() == 1 {
        if let Some((left, right)) = revs[0].split_once("...") {
            let left = if left.is_empty() { "HEAD" } else { left };
            let right = if right.is_empty() { "HEAD" } else { right };
            let left_oid = resolve_revision(&repo, left)
                .with_context(|| format!("unknown revision: '{left}'"))?;
            let right_oid = resolve_revision(&repo, right)
                .with_context(|| format!("unknown revision: '{right}'"))?;
            let bases = grit_lib::rev_list::merge_bases(&repo, left_oid, right_oid, false)?;
            if bases.is_empty() {
                bail!("no merge base between {} and {}", left, right);
            }
            revs = vec![bases[0].to_string(), right_oid.to_string()];
            _symmetric = true;
        } else if let Some((left, right)) = revs[0].split_once("..") {
            let left = if left.is_empty() { "HEAD" } else { left };
            let right = if right.is_empty() { "HEAD" } else { right };
            revs = vec![left.to_owned(), right.to_owned()];
        }
    }
    let work_tree = repo.work_tree.as_deref();

    // Load index (empty if not found)
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Get HEAD tree OID (None if unborn)
    let head_tree = get_head_tree(&repo)?;

    // Determine whether worktree is involved (for content fallback)
    let wt_for_content: Option<&Path> = match (args.cached, revs.len()) {
        (true, _) => None,       // --cached: index vs tree, no worktree
        (false, 0) => work_tree, // unstaged: index vs worktree
        (false, 1) => work_tree, // one rev: tree vs worktree
        (_, 2) => None,          // two revs: tree vs tree
        _ => None,
    };

    let entries: Vec<DiffEntry> = match (args.cached, revs.len()) {
        (true, 0) => {
            // --cached with no revision: index vs HEAD
            diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?
        }
        (true, 1) => {
            // --cached with one revision: index vs that commit's tree
            let tree_oid = commit_or_tree_oid(&repo, &revs[0])?;
            diff_index_to_tree(&repo.odb, &index, Some(&tree_oid))?
        }
        (false, 0) => {
            // No flags: unstaged changes (index vs worktree)
            let wt = work_tree
                .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
            diff_index_to_worktree(&repo.odb, &index, wt)?
        }
        (false, 1) => {
            // One revision: tree vs worktree
            let tree_oid = commit_or_tree_oid(&repo, &revs[0])?;
            let wt = work_tree
                .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
            diff_tree_to_worktree(&repo.odb, Some(&tree_oid), wt, &index)?
        }
        (_, 2) => {
            // Two revisions: tree-to-tree diff
            let tree1 = commit_or_tree_oid(&repo, &revs[0])?;
            let tree2 = commit_or_tree_oid(&repo, &revs[1])?;
            diff_trees(&repo.odb, Some(&tree1), Some(&tree2), "")?
        }
        _ => {
            bail!("too many revisions");
        }
    };

    // Filter by pathspecs
    let entries = filter_by_paths(entries, &paths);

    // Build whitespace mode from flags
    let ws_mode = WhitespaceMode {
        ignore_all_space: args.ignore_all_space,
        ignore_space_change: args.ignore_space_change,
        ignore_space_at_eol: args.ignore_space_at_eol,
        ignore_blank_lines: args.ignore_blank_lines,
        ignore_cr_at_eol: args.ignore_cr_at_eol,
    };

    // When a whitespace-ignore mode is active, filter out entries whose
    // normalised content is identical. Deletions, additions, and mode
    // changes are always reported regardless of whitespace.
    let entries = if ws_mode.any() {
        entries
            .into_iter()
            .filter(|e| {
                // Always keep deletions, additions, and mode changes
                if e.status == DiffStatus::Deleted || e.status == DiffStatus::Added {
                    return true;
                }
                if e.old_mode != e.new_mode {
                    return true;
                }
                let old = read_content(&repo.odb, &e.old_oid, None, e.path());
                let new = read_content(&repo.odb, &e.new_oid, wt_for_content, e.path());
                ws_mode.normalize(&old) != ws_mode.normalize(&new)
            })
            .collect()
    } else {
        entries
    };

    // Apply rename detection if requested (explicit -M flag or diff.renames config).
    let rename_threshold: Option<u32> = if let Some(ref threshold_str) = args.find_renames {
        Some(threshold_str.parse().unwrap_or(50))
    } else {
        // Check diff.renames config.
        use grit_lib::config::ConfigSet;
        let config =
            ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_else(|_| ConfigSet::new());
        match config.get("diff.renames") {
            Some(val) => {
                let val_lower = val.to_lowercase();
                match val_lower.as_str() {
                    "false" | "no" | "0" => None,
                    "true" | "yes" | "1" | "" => Some(50),
                    "copies" | "copy" => Some(50), // -C mode, treat as renames for now
                    _ => None,
                }
            }
            None => Some(50), // Git 2.x defaults diff.renames to true
        }
    };
    let entries = if let Some(threshold) = rename_threshold {
        detect_renames(&repo.odb, entries, threshold)
    } else {
        entries
    };

    // Filter out submodule entries when --ignore-submodules is given.
    let entries = if args.ignore_submodules.is_some() {
        entries
            .into_iter()
            .filter(|e| {
                // Submodule entries have mode 160000
                e.old_mode != "160000" && e.new_mode != "160000"
            })
            .collect()
    } else {
        entries
    };

    // Apply --relative path prefix stripping.
    let entries = if !args.no_relative {
        let prefix = match &args.relative {
            Some(Some(p)) if !p.is_empty() => {
                // --relative=<path> — use the given prefix as a literal prefix.
                // Git does NOT add a trailing '/' — `--relative=sub` matches `subdir/file`.
                Some(p.clone())
            }
            Some(_) => {
                // bare --relative — infer from CWD relative to work tree
                if let Some(wt) = work_tree {
                    if let Ok(cwd) = std::env::current_dir() {
                        if let Ok(rel) = cwd.strip_prefix(wt) {
                            let s = rel.to_string_lossy().to_string();
                            if s.is_empty() {
                                None
                            } else {
                                Some(format!("{s}/"))
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            None => {
                // Check diff.relative config
                use grit_lib::config::ConfigSet;
                let config = ConfigSet::load(Some(&repo.git_dir), false)
                    .unwrap_or_else(|_| ConfigSet::new());
                match config.get("diff.relative") {
                    Some(val) if matches!(val.to_lowercase().as_str(), "true" | "yes" | "1") => {
                        // Infer from CWD
                        if let Some(wt) = work_tree {
                            if let Ok(cwd) = std::env::current_dir() {
                                if let Ok(rel) = cwd.strip_prefix(wt) {
                                    let s = rel.to_string_lossy().to_string();
                                    if s.is_empty() {
                                        None
                                    } else {
                                        Some(format!("{s}/"))
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
        };
        if let Some(ref pfx) = prefix {
            entries
                .into_iter()
                .filter_map(|mut e| {
                    // Filter: at least one path must be under the prefix
                    let old_match = e
                        .old_path
                        .as_ref()
                        .is_some_and(|p| p.starts_with(pfx.as_str()));
                    let new_match = e
                        .new_path
                        .as_ref()
                        .is_some_and(|p| p.starts_with(pfx.as_str()));
                    if !old_match && !new_match {
                        return None;
                    }
                    // Strip prefix from paths, then strip leading '/'
                    if let Some(ref mut p) = e.old_path {
                        if let Some(stripped) = p.strip_prefix(pfx.as_str()) {
                            *p = stripped.trim_start_matches('/').to_owned();
                        }
                    }
                    if let Some(ref mut p) = e.new_path {
                        if let Some(stripped) = p.strip_prefix(pfx.as_str()) {
                            *p = stripped.trim_start_matches('/').to_owned();
                        }
                    }
                    Some(e)
                })
                .collect()
        } else {
            entries
        }
    } else {
        entries
    };

    // Apply orderfile sorting if specified
    let entries = if let Some(ref order_path) = args.order_file {
        apply_orderfile(entries, order_path)
    } else {
        entries
    };

    let has_diff = !entries.is_empty();

    // Determine color mode
    let use_color = match args.color.as_deref() {
        Some("always") => true,
        Some("never") => false,
        Some("auto") | None => io::stdout().is_terminal(),
        Some(_) => false,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let word_diff = args.word_diff.is_some() || args.color_words;

    // Check diff.suppressBlankEmpty config
    let suppress_blank_empty = {
        use grit_lib::config::ConfigSet;
        let config =
            ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_else(|_| ConfigSet::new());
        config
            .get("diff.suppressBlankEmpty")
            .map(|v| v == "true")
            .unwrap_or(false)
    };

    // Load diff.statNameWidth from config if not specified on command line
    if args.stat_name_width.is_none() {
        let snw_config = {
            use grit_lib::config::ConfigSet;
            let cfg =
                ConfigSet::load(Some(&repo.git_dir), false).unwrap_or_else(|_| ConfigSet::new());
            cfg.get("diff.statNameWidth")
                .and_then(|v| v.parse::<usize>().ok())
        };
        if snw_config.is_some() {
            args.stat_name_width = snw_config;
            if !stat_enabled {
                stat_enabled = true;
            }
        }
    }

    // --check: check for whitespace errors
    if args.check {
        let has_errors = check_whitespace_errors(&mut out, &entries, &repo.odb, wt_for_content)?;
        if has_errors {
            std::process::exit(2);
        }
        return Ok(());
    }

    if !args.quiet {
        let context_lines = args.unified.unwrap_or(3);
        if args.shortstat {
            write_shortstat(&mut out, &entries, &repo.odb, wt_for_content)?;
        } else if stat_enabled
            || args.stat_count.is_some()
            || args.stat_width.is_some()
            || args.stat_graph_width.is_some()
            || args.stat_name_width.is_some()
        {
            write_stat(
                &mut out,
                &entries,
                &repo.odb,
                wt_for_content,
                args.stat_count,
                args.stat_width,
                args.stat_name_width,
            )?;
            if args.summary {
                write_diff_summary(&mut out, &entries)?;
            }
        } else if args.raw {
            let oid_len = if args.full_index || args.no_abbrev {
                40
            } else if let Some(n) = args.abbrev {
                n.max(4).min(40)
            } else {
                7
            };
            write_raw(&mut out, &entries, oid_len)?;
        } else if args.numstat {
            write_numstat(&mut out, &entries, &repo.odb, wt_for_content)?;
        } else if args.name_only {
            write_name_only(&mut out, &entries)?;
        } else if args.name_status {
            write_name_status(&mut out, &entries)?;
        } else if args.summary && !stat_enabled {
            write_diff_summary(&mut out, &entries)?;
        } else {
            let patch_abbrev = if args.full_index {
                40
            } else if let Some(n) = args.abbrev {
                n.max(4).min(40)
            } else {
                7
            };
            write_patch_with_prefix(
                &mut out,
                &entries,
                &repo.odb,
                context_lines,
                use_color,
                word_diff,
                wt_for_content,
                suppress_blank_empty,
                patch_abbrev,
                args.inter_hunk_context,
                args.binary,
                &src_prefix,
                &dst_prefix,
            )?;
        }
    }

    if (args.exit_code || args.quiet) && has_diff {
        std::process::exit(1);
    }

    Ok(())
}

/// Split args on `--` to separate revisions from paths.
///
/// Run `diff --no-index <path_a> <path_b>` — compare two files outside a repo.
fn run_no_index(args: &Args) -> Result<()> {
    // Collect paths (skip "--" separators and unrecognized flags)
    let paths: Vec<&String> = args
        .args
        .iter()
        .filter(|a| a.as_str() != "--" && !a.starts_with('-'))
        .collect();
    if paths.len() != 2 {
        bail!("diff --no-index requires exactly two paths");
    }

    let path_a = Path::new(paths[0].as_str());
    let path_b = Path::new(paths[1].as_str());

    // If both paths are directories, diff all files recursively
    if path_a.is_dir() && path_b.is_dir() {
        return run_no_index_dirs(args, path_a, path_b);
    }

    let data_a = std::fs::read(path_a).with_context(|| format!("could not read '{}'", paths[0]))?;
    let data_b = std::fs::read(path_b).with_context(|| format!("could not read '{}'", paths[1]))?;

    if data_a == data_b {
        return Ok(());
    }

    // --quiet / --exit-code: just exit 1 for differences, no output
    if args.quiet {
        std::process::exit(1);
    }

    let text_a = String::from_utf8_lossy(&data_a);
    let text_b = String::from_utf8_lossy(&data_b);
    let context_lines = args.unified.unwrap_or(3);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.name_only {
        writeln!(out, "{}", paths[1])?;
        std::process::exit(1);
    }

    if args.name_status {
        writeln!(out, "M\t{}", paths[1])?;
        std::process::exit(1);
    }

    if args.numstat {
        let (adds, dels) = count_changes(&text_a, &text_b);
        writeln!(out, "{}\t{}\t{}", adds, dels, paths[1])?;
        std::process::exit(1);
    }

    if args.stat.is_some() || args.shortstat {
        let (adds, dels) = count_changes(&text_a, &text_b);
        if args.stat.is_some() {
            let display = if paths[0] != paths[1] {
                format!("{} => {}", paths[0], paths[1])
            } else {
                paths[0].to_string()
            };
            let total = adds + dels;
            writeln!(out, " {} | {}", display, total)?;
        }
        let mut summary = " 1 file changed".to_string();
        if adds > 0 {
            summary.push_str(&format!(
                ", {} insertion{}(+)",
                adds,
                if adds == 1 { "" } else { "s" }
            ));
        }
        if dels > 0 {
            summary.push_str(&format!(
                ", {} deletion{}(-)",
                dels,
                if dels == 1 { "" } else { "s" }
            ));
        }
        writeln!(out, "{summary}")?;
        std::process::exit(1);
    }

    // Determine the effective diff algorithm: last-specified algorithm flag wins.
    let use_anchored = if !args.anchored.is_empty() {
        // Check if a non-anchored algorithm flag appears after --anchored in args
        let raw_args: Vec<String> = std::env::args().collect();
        let last_anchored_pos = raw_args.iter().rposition(|a| a.starts_with("--anchored"));
        let last_other_algo_pos = raw_args.iter().rposition(|a| {
            a == "--patience"
                || a == "--histogram"
                || a == "--minimal"
                || a.starts_with("--diff-algorithm")
        });
        match (last_anchored_pos, last_other_algo_pos) {
            (Some(a), Some(o)) => a > o, // anchored wins only if it's last
            (Some(_), None) => true,
            _ => false,
        }
    } else {
        false
    };
    let diff_output = if use_anchored {
        anchored_unified_diff(
            &text_a,
            &text_b,
            paths[0],
            paths[1],
            context_lines,
            &args.anchored,
        )
    } else {
        unified_diff(&text_a, &text_b, paths[0], paths[1], context_lines)
    };

    // Determine color mode
    let use_color = match args.color.as_deref() {
        Some("always") => true,
        Some("never") => false,
        Some("auto") | None => io::stdout().is_terminal(),
        Some(_) => false,
    };

    if use_color {
        for line in diff_output.lines() {
            if line.starts_with("@@") {
                writeln!(out, "{CYAN}{line}{RESET}")?;
            } else if line.starts_with('+') && !line.starts_with("+++") {
                writeln!(out, "{GREEN}{line}{RESET}")?;
            } else if line.starts_with('-') && !line.starts_with("---") {
                writeln!(out, "{RED}{line}{RESET}")?;
            } else if line.starts_with("diff ")
                || line.starts_with("---")
                || line.starts_with("+++")
            {
                writeln!(out, "{BOLD}{line}{RESET}")?;
            } else {
                writeln!(out, "{line}")?;
            }
        }
    } else {
        write!(out, "{diff_output}")?;
    }

    // Exit with code 1 to indicate differences (like git)
    if args.exit_code || args.quiet {
        std::process::exit(1);
    }
    std::process::exit(1);
}

/// Diff two directories recursively with --no-index.
fn run_no_index_dirs(args: &Args, dir_a: &Path, dir_b: &Path) -> Result<()> {
    use std::collections::BTreeSet;

    fn collect_files(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type()?.is_dir() {
                collect_files(&entry.path(), &rel, out)?;
            } else {
                out.insert(rel);
            }
        }
        Ok(())
    }

    let mut files_a = BTreeSet::new();
    let mut files_b = BTreeSet::new();
    collect_files(dir_a, "", &mut files_a)?;
    collect_files(dir_b, "", &mut files_b)?;

    let all_files: BTreeSet<_> = files_a.iter().chain(files_b.iter()).cloned().collect();
    let mut has_diff = false;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let context_lines = args.unified.unwrap_or(3);

    for rel in &all_files {
        let fa = dir_a.join(rel);
        let fb = dir_b.join(rel);
        let data_a = if fa.is_file() {
            std::fs::read(&fa).ok()
        } else {
            None
        };
        let data_b = if fb.is_file() {
            std::fs::read(&fb).ok()
        } else {
            None
        };

        match (&data_a, &data_b) {
            (Some(a), Some(b)) if a == b => continue,
            _ => {}
        }

        has_diff = true;
        let _label_a = format!("{}/{}", dir_a.display(), rel);
        let label_b = format!("{}/{}", dir_b.display(), rel);

        if args.name_only {
            writeln!(out, "{}", label_b)?;
            continue;
        }
        if args.name_status {
            let status = match (&data_a, &data_b) {
                (None, Some(_)) => "A",
                (Some(_), None) => "D",
                _ => "M",
            };
            writeln!(out, "{}\t{}", status, label_b)?;
            continue;
        }

        let text_a = data_a
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).to_string())
            .unwrap_or_default();
        let text_b = data_b
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).to_string())
            .unwrap_or_default();

        let old_label = if data_a.is_some() {
            format!("a/{}", rel)
        } else {
            "/dev/null".to_string()
        };
        let new_label = if data_b.is_some() {
            format!("b/{}", rel)
        } else {
            "/dev/null".to_string()
        };
        writeln!(out, "diff --git a/{} b/{}", rel, rel)?;
        if data_a.is_none() {
            writeln!(out, "new file mode 100644")?;
        } else if data_b.is_none() {
            writeln!(out, "deleted file mode 100644")?;
        }
        let patch =
            grit_lib::diff::unified_diff(&text_a, &text_b, &old_label, &new_label, context_lines);
        write!(out, "{}", patch)?;
    }

    if has_diff {
        std::process::exit(1);
    }
    Ok(())
}

/// Apply an orderfile to sort diff entries.
///
/// The orderfile contains one pattern per line. Files matching the first
/// pattern come first, then files matching the second, etc. Files not
/// matching any pattern come last in their original order.
/// Apply an orderfile to sort diff entries (public for use by other commands like log).
pub fn apply_orderfile_entries(entries: Vec<DiffEntry>, order_path: &str) -> Vec<DiffEntry> {
    apply_orderfile(entries, order_path)
}

fn apply_orderfile(mut entries: Vec<DiffEntry>, order_path: &str) -> Vec<DiffEntry> {
    let patterns: Vec<String> = match std::fs::read_to_string(order_path) {
        Ok(content) => content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect(),
        Err(_) => return entries,
    };

    // Assign a sort key to each entry based on which pattern it matches first
    let sort_key = |entry: &DiffEntry| -> usize {
        let path = entry
            .new_path
            .as_ref()
            .or(entry.old_path.as_ref())
            .cloned()
            .unwrap_or_default();
        for (i, pat) in patterns.iter().enumerate() {
            if orderfile_pattern_matches(pat, &path) {
                return i;
            }
        }
        patterns.len() // unmatched files go last
    };

    entries.sort_by_key(|e| sort_key(e));
    entries
}

/// Check if an orderfile pattern matches a path.
/// Supports basic glob patterns: `*` matches any sequence, `?` matches one char.
fn orderfile_pattern_matches(pattern: &str, path: &str) -> bool {
    // Simple glob matching: just check if the pattern matches the filename or full path
    let name = path.rsplit('/').next().unwrap_or(path);
    glob_match(pattern, name) || glob_match(pattern, path)
}

/// Basic glob matching (supports `*` and `?`).
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < tb.len() {
        if pi < pb.len() && (pb[pi] == b'?' || pb[pi] == tb[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pb.len() && pb[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pb.len() && pb[pi] == b'*' {
        pi += 1;
    }
    pi == pb.len()
}

/// If `--` is present, everything before is revisions, everything after is paths.
/// Otherwise, we try each arg: if it exists as a file, treat it (and all
/// subsequent args) as paths rather than revisions.
fn parse_rev_and_paths(args: &[String], has_separator: bool) -> (Vec<String>, Vec<String>) {
    if let Some(sep) = args.iter().position(|a| a == "--") {
        let revs = args[..sep].to_vec();
        let paths = args[sep + 1..].to_vec();
        (revs, paths)
    } else if has_separator {
        (Vec::new(), args.to_vec())
    } else {
        // Without `--`, try to guess: if an arg exists as a file/directory,
        // treat it and everything after as paths.
        let mut revs = Vec::new();
        let mut paths = Vec::new();
        let mut in_paths = false;

        for arg in args {
            if in_paths {
                paths.push(arg.clone());
            } else if std::path::Path::new(arg).exists() {
                in_paths = true;
                paths.push(arg.clone());
            } else {
                revs.push(arg.clone());
            }
        }

        (revs, paths)
    }
}

/// Get HEAD's tree OID, or `None` if the repository is unborn.
fn get_head_tree(repo: &Repository) -> Result<Option<ObjectId>> {
    let head_ref = repo.git_dir.join("HEAD");
    let content = match std::fs::read_to_string(&head_ref) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let content = content.trim();

    // Resolve symbolic ref or direct OID
    let oid_str = if let Some(symref) = content.strip_prefix("ref: ") {
        // Try loose ref file first, then fall back to packed-refs
        let ref_path = repo.git_dir.join(symref);
        match std::fs::read_to_string(&ref_path) {
            Ok(s) => s.trim().to_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Look up in packed-refs
                match resolve_packed_ref(&repo.git_dir, symref) {
                    Some(oid) => oid,
                    None => return Ok(None),
                }
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        content.to_owned()
    };

    if oid_str.is_empty() {
        return Ok(None);
    }

    let oid = ObjectId::from_hex(&oid_str).context("parsing HEAD OID")?;
    let obj = repo.odb.read(&oid).context("reading HEAD commit")?;
    let commit = parse_commit(&obj.data).context("parsing HEAD commit")?;
    Ok(Some(commit.tree))
}

/// Look up a ref name in the packed-refs file.
fn resolve_packed_ref(git_dir: &std::path::Path, refname: &str) -> Option<String> {
    let packed = git_dir.join("packed-refs");
    let content = std::fs::read_to_string(packed).ok()?;
    for line in content.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let oid = parts.next()?;
        let name = parts.next()?.trim();
        if name == refname {
            return Some(oid.to_owned());
        }
    }
    None
}

/// Resolve a revision spec to a tree OID, handling both commit and tree objects.
fn commit_or_tree_oid(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let mut oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    loop {
        let obj = repo
            .odb
            .read(&oid)
            .with_context(|| format!("reading object {oid}"))?;
        match obj.kind {
            ObjectKind::Tree => return Ok(oid),
            ObjectKind::Commit => {
                let commit = parse_commit(&obj.data).context("parsing commit")?;
                oid = commit.tree;
            }
            _ => bail!("object '{}' does not name a tree or commit", oid),
        }
    }
}

/// Filter diff entries to only those matching the given pathspecs.
/// Empty pathspecs means include everything.
fn filter_by_paths(entries: Vec<DiffEntry>, paths: &[String]) -> Vec<DiffEntry> {
    if paths.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| {
            let path = e.path();
            paths
                .iter()
                .any(|spec| crate::pathspec::pathspec_matches(spec, path))
        })
        .collect()
}

/// Read content for a diff entry side, falling back to the working tree if
/// the OID is not in the ODB (worktree files are hashed but not stored).
fn read_content(odb: &Odb, oid: &ObjectId, work_tree: Option<&Path>, path: &str) -> String {
    let raw = read_content_raw_or_worktree(odb, oid, work_tree, path);
    String::from_utf8_lossy(&raw).into_owned()
}

/// Read raw bytes for a diff entry side from the ODB.
fn read_content_raw(odb: &Odb, oid: &ObjectId) -> Vec<u8> {
    if *oid == zero_oid() {
        return Vec::new();
    }
    match odb.read(oid) {
        Ok(obj) => obj.data,
        Err(_) => Vec::new(),
    }
}

/// Read raw bytes, falling back to the working tree if the OID isn't in the ODB.
fn read_content_raw_or_worktree(
    odb: &Odb,
    oid: &ObjectId,
    work_tree: Option<&Path>,
    path: &str,
) -> Vec<u8> {
    if *oid == zero_oid() {
        return Vec::new();
    }
    // Try ODB first
    if let Ok(obj) = odb.read(oid) {
        return obj.data;
    }
    // Fall back to reading from working tree
    if let Some(wt) = work_tree {
        if path != "/dev/null" {
            if let Ok(data) = std::fs::read(wt.join(path)) {
                return data;
            }
        }
    }
    Vec::new()
}

/// Check if content appears to be binary (contains NUL bytes in first 8KB).
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// Write a GIT binary patch block (used by --binary).
///
/// Outputs a "GIT binary patch" header followed by a deflated+base85
/// literal representation of the new content, matching git's format.
fn write_git_binary_patch(
    out: &mut impl Write,
    _old_content: &[u8],
    new_content: &[u8],
    old_path: &str,
    new_path: &str,
) -> Result<()> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    writeln!(out, "GIT binary patch")?;
    writeln!(out, "literal {}", new_content.len())?;

    // Deflate the new content
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, new_content)?;
    let compressed = encoder.finish()?;

    // Encode in base85 lines (max 52 raw bytes per line)
    for chunk in compressed.chunks(52) {
        let len_char = (chunk.len() as u8 + b'A' - 1) as char;
        let encoded = base85_encode(chunk);
        writeln!(out, "{len_char}{encoded}")?;
    }
    writeln!(out)?;

    // Reverse patch (literal of old content)
    // For simplicity, output a literal 0 if old is empty
    if _old_content.is_empty() {
        writeln!(out, "literal 0")?;
        writeln!(out, "HcmV?d00001")?;
        writeln!(out)?;
    } else {
        writeln!(out, "literal {}", _old_content.len())?;
        let mut encoder2 = ZlibEncoder::new(Vec::new(), Compression::default());
        std::io::Write::write_all(&mut encoder2, _old_content)?;
        let compressed2 = encoder2.finish()?;
        for chunk in compressed2.chunks(52) {
            let len_char = (chunk.len() as u8 + b'A' - 1) as char;
            let encoded = base85_encode(chunk);
            writeln!(out, "{len_char}{encoded}")?;
        }
        writeln!(out)?;
    }

    let _ = (old_path, new_path); // used in header already
    Ok(())
}

/// Encode bytes in git's base85 format.
fn base85_encode(data: &[u8]) -> String {
    const CHARS: &[u8] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";
    let mut result = String::new();
    for chunk in data.chunks(4) {
        let mut acc: u32 = 0;
        for (i, &byte) in chunk.iter().enumerate() {
            acc |= (byte as u32) << (24 - i * 8);
        }
        // Pad if chunk is less than 4 bytes
        let out_len = match chunk.len() {
            1 => 2,
            2 => 3,
            3 => 4,
            4 => 5,
            _ => unreachable!(),
        };
        let mut buf = [0u8; 5];
        for i in (0..5).rev() {
            buf[i] = CHARS[(acc % 85) as usize];
            acc /= 85;
        }
        for &b in &buf[..out_len] {
            result.push(b as char);
        }
    }
    result
}

/// Write a `diff --git` header plus index/mode lines.
/// Find function context for a hunk header (same logic as grit-lib).
fn find_func_context(header: &str, old_lines: &[&str]) -> Option<String> {
    let at_pos = header.find('-')?;
    let rest = &header[at_pos + 1..];
    let comma_or_space = rest.find([',', ' '])?;
    let start_str = &rest[..comma_or_space];
    let start_line: usize = start_str.parse().ok()?;
    if start_line <= 1 {
        return None;
    }
    let search_end = (start_line - 1).min(old_lines.len());
    for i in (0..search_end).rev() {
        let line = old_lines[i];
        if !line.is_empty() {
            let first = line.as_bytes()[0];
            if first != b' ' && first != b'\t' {
                let truncated = if line.len() > 40 {
                    let mut end = 40;
                    while end > 0 && !line.is_char_boundary(end) {
                        end -= 1;
                    }
                    &line[..end]
                } else {
                    line
                };
                return Some(truncated.to_owned());
            }
        }
    }
    None
}

#[allow(dead_code)]
fn write_diff_header(out: &mut impl Write, entry: &DiffEntry, use_color: bool) -> Result<()> {
    write_diff_header_with_abbrev(out, entry, use_color, 7)
}

#[allow(dead_code)]
fn write_diff_header_with_abbrev(
    out: &mut impl Write,
    entry: &DiffEntry,
    use_color: bool,
    abbrev_len: usize,
) -> Result<()> {
    write_diff_header_with_prefix(out, entry, use_color, abbrev_len, "a/", "b/")
}

fn write_diff_header_with_prefix(
    out: &mut impl Write,
    entry: &DiffEntry,
    use_color: bool,
    abbrev_len: usize,
    src_prefix: &str,
    dst_prefix: &str,
) -> Result<()> {
    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    let (b, r) = if use_color { (BOLD, RESET) } else { ("", "") };
    writeln!(
        out,
        "{b}diff --git {src_prefix}{old_path} {dst_prefix}{new_path}{r}"
    )?;

    let abbr = |oid: &ObjectId| -> String {
        let hex = oid.to_hex();
        let len = abbrev_len.min(hex.len());
        hex[..len].to_owned()
    };

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "{b}new file mode {}{r}", entry.new_mode)?;
            writeln!(
                out,
                "{b}index {}..{}{r}",
                abbr(&entry.old_oid),
                abbr(&entry.new_oid)
            )?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "{b}deleted file mode {}{r}", entry.old_mode)?;
            writeln!(
                out,
                "{b}index {}..{}{r}",
                abbr(&entry.old_oid),
                abbr(&entry.new_oid)
            )?;
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                writeln!(out, "{b}old mode {}{r}", entry.old_mode)?;
                writeln!(out, "{b}new mode {}{r}", entry.new_mode)?;
            }
            if entry.old_mode == entry.new_mode {
                writeln!(
                    out,
                    "{b}index {}..{} {}{r}",
                    abbr(&entry.old_oid),
                    abbr(&entry.new_oid),
                    entry.old_mode
                )?;
            } else {
                writeln!(
                    out,
                    "{b}index {}..{}{r}",
                    abbr(&entry.old_oid),
                    abbr(&entry.new_oid)
                )?;
            }
        }
        DiffStatus::Renamed => {
            let sim = entry.score.unwrap_or(100);
            writeln!(out, "{b}similarity index {sim}%{r}")?;
            writeln!(out, "{b}rename from {old_path}{r}")?;
            writeln!(out, "{b}rename to {new_path}{r}")?;
            if entry.old_oid != entry.new_oid {
                writeln!(
                    out,
                    "{b}index {}..{}{r}",
                    abbr(&entry.old_oid),
                    abbr(&entry.new_oid)
                )?;
            }
        }
        DiffStatus::Copied => {
            let sim = entry.score.unwrap_or(100);
            writeln!(out, "{b}similarity index {sim}%{r}")?;
            writeln!(out, "{b}copy from {old_path}{r}")?;
            writeln!(out, "{b}copy to {new_path}{r}")?;
            if entry.old_oid != entry.new_oid {
                writeln!(
                    out,
                    "{b}index {}..{}{r}",
                    abbr(&entry.old_oid),
                    abbr(&entry.new_oid)
                )?;
            }
        }
        DiffStatus::TypeChanged => {
            writeln!(out, "{b}old mode {}{r}", entry.old_mode)?;
            writeln!(out, "{b}new mode {}{r}", entry.new_mode)?;
        }
        DiffStatus::Unmerged => {}
    }

    Ok(())
}

fn write_patch_with_prefix(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    context_lines: usize,
    use_color: bool,
    word_diff: bool,
    work_tree: Option<&Path>,
    suppress_blank_empty: bool,
    abbrev_len: usize,
    _inter_hunk_context: Option<usize>,
    show_binary: bool,
    src_prefix: &str,
    dst_prefix: &str,
) -> Result<()> {
    for entry in entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");

        write_diff_header_with_prefix(out, entry, use_color, abbrev_len, src_prefix, dst_prefix)?;

        // Check for binary content
        let old_content_raw = read_content_raw(odb, &entry.old_oid);
        let new_content_raw =
            read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, new_path);

        if is_binary(&old_content_raw) || is_binary(&new_content_raw) {
            if show_binary {
                // --binary: output a "GIT binary patch" block
                write_git_binary_patch(
                    out,
                    &old_content_raw,
                    &new_content_raw,
                    old_path,
                    new_path,
                )?;
            } else {
                writeln!(
                    out,
                    "Binary files {}{} and {}{} differ",
                    src_prefix, old_path, dst_prefix, new_path
                )?;
            }
            continue;
        }

        let old_content = String::from_utf8_lossy(&old_content_raw).into_owned();
        let new_content = String::from_utf8_lossy(&new_content_raw).into_owned();

        // For Added files, show --- /dev/null; for Deleted files, show +++ /dev/null
        let display_old = if entry.status == DiffStatus::Added {
            "/dev/null"
        } else {
            old_path
        };
        let display_new = if entry.status == DiffStatus::Deleted {
            "/dev/null"
        } else {
            new_path
        };

        if word_diff {
            let patch = word_diff_output(
                &old_content,
                &new_content,
                display_old,
                display_new,
                context_lines,
            );
            let patch = if suppress_blank_empty {
                strip_blank_context_trailing_space(&patch)
            } else {
                patch
            };
            if use_color {
                write_colored_patch(out, &patch)?;
            } else {
                write!(out, "{patch}")?;
            }
        } else {
            let patch = grit_lib::diff::unified_diff_with_prefix(
                &old_content,
                &new_content,
                display_old,
                display_new,
                context_lines,
                src_prefix,
                dst_prefix,
            );
            let patch = if suppress_blank_empty {
                strip_blank_context_trailing_space(&patch)
            } else {
                patch
            };

            if use_color {
                write_colored_patch(out, &patch)?;
            } else {
                write!(out, "{patch}")?;
            }
        }
    }
    Ok(())
}

/// Strip trailing space from blank context lines in unified diff output.
///
/// When `diff.suppressBlankEmpty` is true, context lines that consist of
/// only a single space (empty context line) should become truly empty lines.
fn strip_blank_context_trailing_space(patch: &str) -> String {
    let mut result = String::with_capacity(patch.len());
    for line in patch.split_inclusive('\n') {
        if line == " \n" {
            result.push('\n');
        } else {
            result.push_str(line);
        }
    }
    result
}

/// Write a unified diff patch with ANSI color codes.
fn write_colored_patch(out: &mut impl Write, patch: &str) -> Result<()> {
    for line in patch.lines() {
        if line.starts_with("---") || line.starts_with("+++") {
            writeln!(out, "{BOLD}{line}{RESET}")?;
        } else if line.starts_with("@@") {
            writeln!(out, "{CYAN}{line}{RESET}")?;
        } else if line.starts_with('-') {
            writeln!(out, "{RED}{line}{RESET}")?;
        } else if line.starts_with('+') {
            writeln!(out, "{GREEN}{line}{RESET}")?;
        } else {
            writeln!(out, "{line}{RESET}")?;
        }
    }
    Ok(())
}

/// Generate word-level diff output with `[-removed-]{+added+}` markers.
fn word_diff_output(
    old_content: &str,
    new_content: &str,
    old_path: &str,
    new_path: &str,
    context_lines: usize,
) -> String {
    use similar::{ChangeTag, TextDiff};

    let mut output = String::new();
    output.push_str(&format!("--- a/{old_path}\n"));
    output.push_str(&format!("+++ b/{new_path}\n"));

    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let line_diff = TextDiff::from_slices(&old_lines, &new_lines);

    for hunk in line_diff
        .unified_diff()
        .context_radius(context_lines)
        .iter_hunks()
    {
        // Write the hunk header with function context.
        let hunk_str = format!("{hunk}");
        if let Some(header_end) = hunk_str.find('\n') {
            let header = &hunk_str[..header_end];
            output.push_str(header);
            // Add function context (like Git does).
            if let Some(func) = find_func_context(header, &old_lines) {
                output.push(' ');
                output.push_str(&func);
            }
            output.push('\n');
        }

        // Process each change in the hunk
        for change in hunk.iter_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    output.push_str(change.value());
                    output.push('\n');
                }
                ChangeTag::Delete => {
                    // Look for a corresponding insert to do word-level diff
                    // For simplicity, output the whole line as deleted
                    // We'll handle paired changes below
                }
                ChangeTag::Insert => {}
            }
        }
    }

    // Simpler approach: use the similar crate's word-level diff directly
    output.clear();
    output.push_str(&format!("--- a/{old_path}\n"));
    output.push_str(&format!("+++ b/{new_path}\n"));

    // Build unified diff with word-level changes within each hunk
    let line_diff = TextDiff::from_lines(old_content, new_content);

    for hunk in line_diff
        .unified_diff()
        .context_radius(context_lines)
        .iter_hunks()
    {
        // Write hunk header with function context.
        let hunk_str = format!("{hunk}");
        if let Some(header_end) = hunk_str.find('\n') {
            let header = &hunk_str[..header_end];
            output.push_str(header);
            let old_lines_for_ctx: Vec<&str> = old_content.lines().collect();
            if let Some(func) = find_func_context(header, &old_lines_for_ctx) {
                output.push(' ');
                output.push_str(&func);
            }
            output.push('\n');
        }

        // Collect changes and pair deletions with insertions
        let changes: Vec<_> = hunk.iter_changes().collect();
        let mut i = 0;
        while i < changes.len() {
            let change = &changes[i];
            match change.tag() {
                ChangeTag::Equal => {
                    let val = change.value();
                    // Strip trailing newline for output
                    let line = val.strip_suffix('\n').unwrap_or(val);
                    output.push_str(line);
                    output.push('\n');
                    i += 1;
                }
                ChangeTag::Delete => {
                    // Collect consecutive deletions
                    let mut del_lines = Vec::new();
                    while i < changes.len() && changes[i].tag() == ChangeTag::Delete {
                        del_lines.push(changes[i].value());
                        i += 1;
                    }
                    // Collect consecutive insertions
                    let mut ins_lines = Vec::new();
                    while i < changes.len() && changes[i].tag() == ChangeTag::Insert {
                        ins_lines.push(changes[i].value());
                        i += 1;
                    }

                    // Do word-level diff between paired del/ins
                    let del_text: String = del_lines.join("");
                    let ins_text: String = ins_lines.join("");

                    if ins_lines.is_empty() {
                        // Pure deletion
                        let text = del_text.strip_suffix('\n').unwrap_or(&del_text);
                        output.push_str(&format!("[-{text}-]"));
                        output.push('\n');
                    } else if del_lines.is_empty() {
                        // Pure insertion
                        let text = ins_text.strip_suffix('\n').unwrap_or(&ins_text);
                        output.push_str(&format!("{{+{text}+}}"));
                        output.push('\n');
                    } else {
                        // Word-level diff
                        let word_diff = TextDiff::from_words(&del_text, &ins_text);
                        for word_change in word_diff.iter_all_changes() {
                            let val = word_change.value();
                            match word_change.tag() {
                                ChangeTag::Equal => output.push_str(val),
                                ChangeTag::Delete => {
                                    output.push_str(&format!("[-{val}-]"));
                                }
                                ChangeTag::Insert => {
                                    output.push_str(&format!("{{+{val}+}}"));
                                }
                            }
                        }
                        // Ensure newline at end
                        if !output.ends_with('\n') {
                            output.push('\n');
                        }
                    }
                }
                ChangeTag::Insert => {
                    // Orphan insert (no preceding delete)
                    let text = change.value();
                    let line = text.strip_suffix('\n').unwrap_or(text);
                    output.push_str(&format!("{{+{line}+}}"));
                    output.push('\n');
                    i += 1;
                }
            }
        }
    }

    output
}

/// Write only the summary line: `N files changed, N insertions(+), N deletions(-)`.
fn write_shortstat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    let mut files_changed = 0usize;

    for entry in entries {
        let old_content = read_content(odb, &entry.old_oid, None, entry.path());
        let new_content = read_content(odb, &entry.new_oid, work_tree, entry.path());
        let (ins, del) = count_changes(&old_content, &new_content);
        total_ins += ins;
        total_del += del;
        files_changed += 1;
    }

    let mut summary = format!(
        " {} file{} changed",
        files_changed,
        if files_changed == 1 { "" } else { "s" }
    );
    append_stat_counts(&mut summary, total_ins, total_del);
    writeln!(out, "{summary}")?;

    Ok(())
}

/// Append insertions/deletions counts to a summary string.
/// Git only shows insertions/deletions when they are non-zero,
/// except when both are zero (e.g. mode-only changes).
fn append_stat_counts(summary: &mut String, total_ins: usize, total_del: usize) {
    if total_ins > 0 {
        summary.push_str(&format!(
            ", {} insertion{}(+)",
            total_ins,
            if total_ins == 1 { "" } else { "s" }
        ));
    }
    if total_del > 0 {
        summary.push_str(&format!(
            ", {} deletion{}(-)",
            total_del,
            if total_del == 1 { "" } else { "s" }
        ));
    }
    if total_ins == 0 && total_del == 0 {
        summary.push_str(", 0 insertions(+), 0 deletions(-)");
    }
}

/// Get the terminal width, defaulting to 80 if unavailable.
fn terminal_width() -> usize {
    // Try COLUMNS env var first
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<usize>() {
            if w > 0 {
                return w;
            }
        }
    }
    // Try `stty size` which outputs "rows cols"
    if let Ok(output) = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::null())
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() == 2 {
            if let Ok(w) = parts[1].parse::<usize>() {
                if w > 0 {
                    return w;
                }
            }
        }
    }
    80
}

/// Format a single `--stat` line matching git's output format.
///
/// Git scales all bars relative to the largest change count across
/// all files, fitting within the available bar width.
///
/// `max_change` is the largest (insertions + deletions) across all entries.
/// `max_bar` is the available character width for the histogram bar.
fn format_stat_line_git(
    path: &str,
    insertions: usize,
    deletions: usize,
    max_path_len: usize,
    count_width: usize,
    max_change: usize,
    max_bar: usize,
) -> String {
    let total = insertions + deletions;
    // Scale all bars relative to the largest change
    let (plus_count, minus_count) = if max_change <= max_bar {
        // No scaling needed — bars fit as-is
        (insertions, deletions)
    } else {
        // Scale proportionally, ensuring at least 1 char for non-zero sides
        let scale = max_bar as f64 / max_change as f64;
        let plus = if insertions == 0 {
            0
        } else {
            (insertions as f64 * scale).round().max(1.0) as usize
        };
        let minus = if deletions == 0 {
            0
        } else {
            (deletions as f64 * scale).round().max(1.0) as usize
        };
        // Clamp total to max_bar
        let sum = plus + minus;
        if sum > max_bar {
            // Shrink the larger side
            if plus >= minus {
                (max_bar.saturating_sub(minus), minus)
            } else {
                (plus, max_bar.saturating_sub(plus))
            }
        } else {
            (plus, minus)
        }
    };
    let plus = "+".repeat(plus_count);
    let minus = "-".repeat(minus_count);
    // Pad path to max_path_len display columns (not bytes)
    let path_display_width = UnicodeWidthStr::width(path);
    let padding = max_path_len.saturating_sub(path_display_width);
    let bar = format!("{plus}{minus}");
    if bar.is_empty() {
        format!(
            " {}{} | {:>cw$}",
            path,
            " ".repeat(padding),
            total,
            cw = count_width,
        )
    } else {
        format!(
            " {}{} | {:>cw$} {bar}",
            path,
            " ".repeat(padding),
            total,
            cw = count_width,
        )
    }
}

/// Write a stat summary for each entry, followed by a totals line.
fn write_stat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
    stat_count: Option<usize>,
    stat_width: Option<usize>,
    stat_name_width: Option<usize>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    // Build display paths (compact rename format for renames).
    let display_paths: Vec<String> = entries
        .iter()
        .map(|e| match e.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let old = e.old_path.as_deref().unwrap_or("");
                let new = e.new_path.as_deref().unwrap_or("");
                grit_lib::diff::format_rename_path(old, new)
            }
            _ => e.path().to_owned(),
        })
        .collect();
    let max_path_len = display_paths
        .iter()
        .map(|p| UnicodeWidthStr::width(p.as_str()))
        .max()
        .unwrap_or(0);

    // Collect per-file stats first so we can compute the count column width
    let mut file_stats: Vec<(&str, usize, usize)> = Vec::new();
    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    let mut files_changed = 0usize;

    for (i, entry) in entries.iter().enumerate() {
        let old_content = read_content(odb, &entry.old_oid, None, entry.path());
        let new_content = read_content(odb, &entry.new_oid, work_tree, entry.path());
        let (ins, del) = count_changes(&old_content, &new_content);
        file_stats.push((&display_paths[i], ins, del));
        total_ins += ins;
        total_del += del;
        files_changed += 1;
    }

    // Compute the width for the count column (like git does)
    let max_count = file_stats.iter().map(|(_, i, d)| i + d).max().unwrap_or(0);
    let count_width = format!("{}", max_count).len();

    // Compute layout widths from total width, like git.
    // Line format: " {name:<N} | {count:>C} {bar}"
    // Total chars = 1 + N + 3 + C + 1 + bar_len = N + C + 5 + bar_len
    // Target total = total_width - 1
    let total_width = stat_width.unwrap_or_else(terminal_width);
    let overhead = count_width + 5; // " " + " | " + " " before bar = 1+3+1 = 5
    let line_budget = total_width.saturating_sub(1).saturating_sub(overhead);
    // line_budget = name_len + bar_len

    // Apply stat_name_width if set, or truncate to fit terminal width
    let max_path_len = if let Some(nw) = stat_name_width {
        max_path_len.min(nw)
    } else if max_path_len > line_budget.saturating_sub(1) {
        // Name too long for the budget — truncate, leaving at least 1 char for bar
        line_budget.saturating_sub(1)
    } else {
        max_path_len
    };

    let max_bar = line_budget.saturating_sub(max_path_len).max(10);

    let display_stats: &[(&str, usize, usize)] = if let Some(limit) = stat_count {
        if file_stats.len() > limit {
            &file_stats[..limit]
        } else {
            &file_stats
        }
    } else {
        &file_stats
    };
    for (path, ins, del) in display_stats {
        // Truncate path if its display width exceeds max_path_len
        let path_width = UnicodeWidthStr::width(*path);
        let display_path: std::borrow::Cow<str> = if path_width > max_path_len {
            // Git truncates with "..." prefix, keeping as much of the suffix as fits
            let target_suffix_width = max_path_len.saturating_sub(3);
            // Walk from the end, accumulating display width
            let mut width_acc = 0usize;
            let mut cut_idx = path.len();
            for (idx, ch) in path.char_indices().rev() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if width_acc + cw > target_suffix_width {
                    break;
                }
                width_acc += cw;
                cut_idx = idx;
            }
            let suffix = &path[cut_idx..];
            std::borrow::Cow::Owned(format!("...{}", suffix))
        } else {
            std::borrow::Cow::Borrowed(*path)
        };
        let line = format_stat_line_git(
            &display_path,
            *ins,
            *del,
            max_path_len,
            count_width,
            max_count,
            max_bar,
        );
        writeln!(out, "{line}")?;
    }
    if let Some(limit) = stat_count {
        if file_stats.len() > limit {
            writeln!(out, " ...")?;
        }
    }

    // Summary line
    let mut summary = format!(
        " {} file{} changed",
        files_changed,
        if files_changed == 1 { "" } else { "s" }
    );
    append_stat_counts(&mut summary, total_ins, total_del);
    writeln!(out, "{summary}")?;

    Ok(())
}

/// Write machine-readable numstat output: `{insertions}\t{deletions}\t{path}`.
fn write_numstat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
) -> Result<()> {
    for entry in entries {
        let old_content = read_content(odb, &entry.old_oid, None, entry.path());
        let new_content = read_content(odb, &entry.new_oid, work_tree, entry.path());
        let (ins, del) = count_changes(&old_content, &new_content);
        writeln!(out, "{ins}\t{del}\t{}", entry.path())?;
    }
    Ok(())
}

/// Write only the names of changed files.
/// Write `--summary` output for rename/copy/mode-change entries.
fn write_diff_summary(out: &mut impl Write, entries: &[DiffEntry]) -> Result<()> {
    use grit_lib::diff::format_rename_path;
    for entry in entries {
        match entry.status {
            DiffStatus::Renamed => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let compact = format_rename_path(old, new);
                let sim = entry.score.unwrap_or(100);
                writeln!(out, " rename {compact} ({sim}%)")?;
            }
            DiffStatus::Copied => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let compact = format_rename_path(old, new);
                let sim = entry.score.unwrap_or(100);
                writeln!(out, " copy {compact} ({sim}%)")?;
            }
            DiffStatus::Added => {
                writeln!(out, " create mode {} {}", entry.new_mode, entry.path())?;
            }
            DiffStatus::Deleted => {
                writeln!(out, " delete mode {} {}", entry.old_mode, entry.path())?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_name_only(out: &mut impl Write, entries: &[DiffEntry]) -> Result<()> {
    for entry in entries {
        writeln!(out, "{}", entry.path())?;
    }
    Ok(())
}

/// Write `{status_letter}\t{path}` for each entry.
/// For renames/copies, output `R100\told_path\tnew_path`.
/// Write raw diff format: `:old-mode new-mode old-oid new-oid status\tpath`
fn write_raw(out: &mut impl Write, entries: &[DiffEntry], abbrev_len: usize) -> Result<()> {
    for entry in entries {
        let old_mode = &entry.old_mode;
        let new_mode = &entry.new_mode;
        let old_oid_hex = entry.old_oid.to_hex();
        let new_oid_hex = entry.new_oid.to_hex();
        let olen = abbrev_len.min(old_oid_hex.len());
        let nlen = abbrev_len.min(new_oid_hex.len());
        let old_oid = &old_oid_hex[..olen];
        let new_oid = &new_oid_hex[..nlen];
        let status = entry.status.letter();
        match entry.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let score = entry.score.unwrap_or(100);
                let old_path = entry.old_path.as_deref().unwrap_or("");
                let new_path = entry.new_path.as_deref().unwrap_or("");
                writeln!(out, ":{old_mode} {new_mode} {old_oid} {new_oid} {status}{score:03}\t{old_path}\t{new_path}")?;
            }
            _ => {
                writeln!(
                    out,
                    ":{old_mode} {new_mode} {old_oid} {new_oid} {status}\t{}",
                    entry.path()
                )?;
            }
        }
    }
    Ok(())
}

fn write_name_status(out: &mut impl Write, entries: &[DiffEntry]) -> Result<()> {
    for entry in entries {
        match entry.status {
            DiffStatus::Renamed => {
                let s = entry.score.unwrap_or(100);
                writeln!(
                    out,
                    "R{:03}\t{}\t{}",
                    s,
                    entry.old_path.as_deref().unwrap_or(""),
                    entry.new_path.as_deref().unwrap_or("")
                )?;
            }
            DiffStatus::Copied => {
                let s = entry.score.unwrap_or(100);
                writeln!(
                    out,
                    "C{:03}\t{}\t{}",
                    s,
                    entry.old_path.as_deref().unwrap_or(""),
                    entry.new_path.as_deref().unwrap_or("")
                )?;
            }
            _ => {
                writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
            }
        }
    }
    Ok(())
}

/// Check for whitespace errors in added/modified lines.
/// Returns true if any errors were found.
fn check_whitespace_errors(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
) -> Result<bool> {
    use grit_lib::diff::zero_oid;
    let mut has_errors = false;

    for entry in entries {
        if entry.status == DiffStatus::Deleted {
            continue;
        }
        let path = entry.path();

        // Read old and new content
        let old_content = if entry.old_oid == zero_oid() {
            String::new()
        } else {
            read_content(
                odb,
                &entry.old_oid,
                work_tree,
                entry.old_path.as_deref().unwrap_or(path),
            )
        };
        let new_content = if entry.new_oid == zero_oid() {
            String::new()
        } else {
            read_content(odb, &entry.new_oid, work_tree, path)
        };

        // Compute diff and check added lines for whitespace errors
        use similar::{ChangeTag, TextDiff};
        let diff = TextDiff::from_lines(&old_content, &new_content);
        let mut line_no = 0u64;
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Insert => {
                    line_no += 1;
                    let line = change.value();
                    // Check for trailing whitespace
                    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                    if trimmed != trimmed.trim_end() {
                        writeln!(out, "{}:{}: trailing whitespace.", path, line_no)?;
                        write!(out, "+{}", line)?;
                        if !line.ends_with('\n') {
                            writeln!(out)?;
                        }
                        has_errors = true;
                    }
                    // Check for space before tab in indent
                    let indent: &str = &trimmed[..trimmed.len() - trimmed.trim_start().len()];
                    if indent.contains(" \t") {
                        writeln!(out, "{}:{}: space before tab in indent.", path, line_no)?;
                        write!(out, "+{}", line)?;
                        if !line.ends_with('\n') {
                            writeln!(out)?;
                        }
                        has_errors = true;
                    }
                }
                ChangeTag::Equal => {
                    line_no += 1;
                }
                ChangeTag::Delete => {}
            }
        }
    }
    Ok(has_errors)
}

/// Resolve the source and destination prefixes for diff output, considering
/// command-line flags and config options.
fn resolve_diff_prefixes(args: &Args, repo: &Repository) -> (String, String) {
    if args.default_prefix {
        return ("a/".to_owned(), "b/".to_owned());
    }
    if args.no_prefix {
        return (String::new(), String::new());
    }
    let explicit_src = args.src_prefix.is_some();
    let explicit_dst = args.dst_prefix.is_some();
    if explicit_src && explicit_dst {
        return (
            args.src_prefix.clone().unwrap_or_default(),
            args.dst_prefix.clone().unwrap_or_default(),
        );
    }

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();

    if let Some(ref cfg) = config {
        if let Some(ref val) = cfg.get("diff.noprefix") {
            if val == "true" || val == "yes" || val == "on" || val == "1" {
                return (String::new(), String::new());
            }
        }
        if let Some(ref val) = cfg.get("diff.mnemonicprefix") {
            if val == "true" || val == "yes" || val == "on" || val == "1" {
                return ("i/".to_owned(), "w/".to_owned());
            }
        }
    }

    let src = if explicit_src {
        args.src_prefix.clone().unwrap_or_default()
    } else if let Some(ref cfg) = config {
        cfg.get("diff.srcprefix").unwrap_or_else(|| "a/".to_owned())
    } else {
        "a/".to_owned()
    };

    let dst = if explicit_dst {
        args.dst_prefix.clone().unwrap_or_default()
    } else if let Some(ref cfg) = config {
        cfg.get("diff.dstprefix").unwrap_or_else(|| "b/".to_owned())
    } else {
        "b/".to_owned()
    };

    (src, dst)
}
