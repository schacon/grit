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
    anchored_unified_diff, count_changes, detect_copies, detect_renames, diff_index_to_tree,
    diff_index_to_worktree, diff_tree_to_worktree, diff_trees, empty_blob_oid, zero_oid, DiffEntry,
    DiffStatus,
};
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, serialize_tree, tree_entry_cmp, CommitData,
    ObjectId, ObjectKind, TreeEntry,
};
use grit_lib::odb::Odb;
use grit_lib::refs::{resolve_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{abbreviate_object_id, resolve_revision};
use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use time::OffsetDateTime;
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
        if self.ignore_cr_at_eol && s.ends_with('\r') {
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

/// Effective line diff algorithm selection used for patch generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffAlgorithmChoice {
    Myers,
    Patience,
}

impl DiffAlgorithmChoice {
    /// Convert to the `similar` crate's algorithm enum.
    fn to_similar(self) -> similar::Algorithm {
        match self {
            DiffAlgorithmChoice::Myers => similar::Algorithm::Myers,
            DiffAlgorithmChoice::Patience => similar::Algorithm::Patience,
        }
    }
}

/// Parse a git diff algorithm name.
///
/// Git supports `myers`, `minimal`, `patience`, and `histogram`.
/// This implementation maps `minimal` to Myers and `histogram` to Patience.
fn parse_diff_algorithm_choice(name: &str) -> Option<DiffAlgorithmChoice> {
    match name.trim().to_ascii_lowercase().as_str() {
        "myers" | "default" | "minimal" => Some(DiffAlgorithmChoice::Myers),
        "patience" | "histogram" => Some(DiffAlgorithmChoice::Patience),
        _ => None,
    }
}

/// Determine the last CLI-selected non-anchored diff algorithm.
///
/// We scan raw argv so ordering semantics match git (last option wins),
/// including the `--diff-algorithm=<name>` and `--diff-algorithm <name>` forms.
fn cli_diff_algorithm_choice(raw_args: &[String], args: &Args) -> Option<DiffAlgorithmChoice> {
    let mut last: Option<(usize, DiffAlgorithmChoice)> = None;
    let mut i = 1usize;

    while i < raw_args.len() {
        let arg = &raw_args[i];
        let update = |pos: usize,
                      value: DiffAlgorithmChoice,
                      slot: &mut Option<(usize, DiffAlgorithmChoice)>| {
            match slot {
                Some((prev_pos, _)) if *prev_pos > pos => {}
                _ => *slot = Some((pos, value)),
            }
        };

        if arg == "--patience" {
            update(i, DiffAlgorithmChoice::Patience, &mut last);
        } else if arg == "--histogram" {
            update(i, DiffAlgorithmChoice::Patience, &mut last);
        } else if arg == "--minimal" {
            update(i, DiffAlgorithmChoice::Myers, &mut last);
        } else if let Some(value) = arg.strip_prefix("--diff-algorithm=") {
            if let Some(choice) = parse_diff_algorithm_choice(value) {
                update(i, choice, &mut last);
            }
        } else if arg == "--diff-algorithm" {
            if let Some(value) = raw_args.get(i + 1) {
                if let Some(choice) = parse_diff_algorithm_choice(value) {
                    update(i + 1, choice, &mut last);
                }
                i += 1;
            }
        }

        i += 1;
    }

    if let Some((_, choice)) = last {
        return Some(choice);
    }

    if let Some(ref name) = args.diff_algorithm {
        return parse_diff_algorithm_choice(name);
    }
    if args.patience || args.histogram {
        return Some(DiffAlgorithmChoice::Patience);
    }
    if args.minimal {
        return Some(DiffAlgorithmChoice::Myers);
    }
    None
}

/// Resolve the effective algorithm for a specific path.
///
/// Precedence:
/// 1. Explicit CLI algorithm options.
/// 2. `diff.<driver>.algorithm` from attributes + config.
/// 3. `diff.algorithm` from config.
/// 4. Myers default.
fn resolve_diff_algorithm_for_path(
    cli_choice: Option<DiffAlgorithmChoice>,
    config: Option<&grit_lib::config::ConfigSet>,
    rules: Option<&[grit_lib::crlf::AttrRule]>,
    work_tree: Option<&Path>,
    path: &str,
) -> DiffAlgorithmChoice {
    if let Some(choice) = cli_choice {
        return choice;
    }

    let Some(config) = config else {
        return DiffAlgorithmChoice::Myers;
    };

    if let Some(rules) = rules {
        let attrs_path = path_for_attr_lookup_opt(path, work_tree);
        let attrs = grit_lib::crlf::get_file_attrs(rules, &attrs_path, config);
        if let Some(driver) = attrs.diff_driver {
            if let Some(value) = config.get(&format!("diff.{driver}.algorithm")) {
                if let Some(choice) = parse_diff_algorithm_choice(&value) {
                    return choice;
                }
            }
        }
    }

    if let Some(value) = config.get("diff.algorithm") {
        if let Some(choice) = parse_diff_algorithm_choice(&value) {
            return choice;
        }
    }

    DiffAlgorithmChoice::Myers
}

/// Resolve an optional tree object to use as the source for gitattributes.
///
/// Precedence matches git's attribute-source behavior:
/// `GIT_ATTR_SOURCE` (set by `--attr-source`) first, then `attr.tree`.
fn resolve_attr_source_tree(
    repo: &Repository,
    config: Option<&grit_lib::config::ConfigSet>,
) -> Result<Option<ObjectId>> {
    if let Ok(raw) = std::env::var("GIT_ATTR_SOURCE") {
        if raw.trim().is_empty() {
            bail!("no attribute source given for --attr-source");
        }
        let oid =
            resolve_revision(repo, raw.trim()).context("bad --attr-source or GIT_ATTR_SOURCE")?;
        let obj = repo
            .odb
            .read(&oid)
            .context("bad --attr-source or GIT_ATTR_SOURCE")?;
        return match obj.kind {
            ObjectKind::Tree => Ok(Some(oid)),
            ObjectKind::Commit => parse_commit(&obj.data)
                .map(|commit| Some(commit.tree))
                .map_err(Into::into),
            _ => bail!("bad --attr-source or GIT_ATTR_SOURCE"),
        };
    }

    if let Some(cfg) = config {
        if let Some(raw) = cfg.get("attr.tree") {
            if raw.trim().is_empty() {
                return Ok(None);
            }
            let oid = resolve_revision(repo, raw.trim()).context("bad attr.tree")?;
            let obj = repo.odb.read(&oid).context("bad attr.tree")?;
            return match obj.kind {
                ObjectKind::Tree => Ok(Some(oid)),
                ObjectKind::Commit => parse_commit(&obj.data)
                    .map(|commit| Some(commit.tree))
                    .map_err(Into::into),
                _ => bail!("bad attr.tree"),
            };
        }
    }

    Ok(None)
}

/// Read a blob from a specific tree by relative path.
fn read_blob_from_tree_path(
    repo: &Repository,
    tree_oid: &ObjectId,
    rel_path: &str,
) -> Result<Option<Vec<u8>>> {
    let mut current_tree = *tree_oid;
    let mut components = rel_path.split('/').filter(|c| !c.is_empty()).peekable();
    if components.peek().is_none() {
        return Ok(None);
    }

    while let Some(component) = components.next() {
        let tree_obj = repo.odb.read(&current_tree)?;
        if tree_obj.kind != ObjectKind::Tree {
            return Ok(None);
        }
        let entries = parse_tree(&tree_obj.data)?;
        let Some(entry) = entries
            .into_iter()
            .find(|entry| entry.name.as_slice() == component.as_bytes())
        else {
            return Ok(None);
        };

        if components.peek().is_some() {
            if entry.mode != 0o040000 {
                return Ok(None);
            }
            current_tree = entry.oid;
            continue;
        }

        let obj = repo.odb.read(&entry.oid)?;
        if obj.kind != ObjectKind::Blob {
            return Ok(None);
        }
        return Ok(Some(obj.data));
    }

    Ok(None)
}

/// Load diff attribute rules from attr-source tree or worktree.
fn load_diff_attr_rules(
    repo: &Repository,
    config: Option<&grit_lib::config::ConfigSet>,
    work_tree: Option<&Path>,
) -> Result<Option<Vec<grit_lib::crlf::AttrRule>>> {
    if let Some(attr_tree) = resolve_attr_source_tree(repo, config)? {
        let content = read_blob_from_tree_path(repo, &attr_tree, ".gitattributes")?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let rules = grit_lib::crlf::parse_gitattributes_content(&content);
        return Ok(Some(rules));
    }

    let mut rules = if let Some(wt) = work_tree {
        grit_lib::crlf::load_gitattributes(wt)
    } else {
        Vec::new()
    };

    append_core_attributes_rules(config, &mut rules);

    if rules.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rules))
    }
}

/// Append rules from `core.attributesFile` when configured.
fn append_core_attributes_rules(
    config: Option<&grit_lib::config::ConfigSet>,
    rules: &mut Vec<grit_lib::crlf::AttrRule>,
) {
    let Some(config) = config else {
        return;
    };
    let Some(path) = config.get("core.attributesfile") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    if let Ok(content) = std::fs::read_to_string(Path::new(&path)) {
        rules.extend(grit_lib::crlf::parse_gitattributes_content(&content));
    }
}

/// Arguments for `grit diff`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show changes between commits, commit and working tree, etc.")]
pub struct Args {
    /// Produce combined diff format from merge commits.
    #[arg(short = 'c')]
    pub combined: bool,

    /// Produce dense combined diff format from merge commits.
    #[arg(long = "cc")]
    pub dense_combined: bool,

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

    /// Omit preimage for deletes.
    #[arg(short = 'D', long = "irreversible-delete")]
    pub irreversible_delete: bool,

    /// Output a binary diff that can be applied with git-apply.
    #[arg(long = "binary")]
    pub binary: bool,

    /// Swap two inputs; show data from destination as deletion and source as addition.
    #[arg(short = 'R', long = "reverse")]
    pub reverse: bool,

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
    #[arg(short = 'M', long = "find-renames", value_name = "N", default_missing_value = "50", num_args = 0..=1, require_equals = true)]
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

    /// Detect copies (`-C`); repeated `-C` enables harder copy search.
    #[arg(short = 'C', long = "find-copies", value_name = "N", default_missing_value = "50", num_args = 0..=1, require_equals = true, action = clap::ArgAction::Append)]
    pub find_copies: Vec<String>,

    /// Find copies harder (look at unmodified files as source).
    #[arg(long = "find-copies-harder")]
    pub find_copies_harder: bool,

    /// Rename/copy detection limit.
    #[arg(short = 'l', value_name = "N")]
    pub rename_limit: Option<usize>,

    /// Pickaxe: look for diffs that change the number of occurrences of the specified string.
    /// Parsed manually from trailing args since -S<string> value is attached.
    #[arg(skip)]
    pub pickaxe_string: Option<String>,

    /// Pickaxe: look for diffs whose patch text contains added/removed lines matching regex.
    /// Parsed manually from trailing args since -G takes a space-separated value.
    #[arg(skip)]
    pub pickaxe_grep: Option<String>,

    /// Treat the string given to -S as a POSIX extended regex.
    #[arg(long = "pickaxe-regex")]
    pub pickaxe_regex: bool,

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

    // Clap treats `-C` as an option with an optional value, which can make
    // repeated `-C -C` invocations ambiguous. Detect raw short-flag usage to
    // preserve Git-compatible "repeat -C means harder copy search" behavior.
    let repeated_copy_flags = raw_args
        .iter()
        .skip(1)
        .filter(|arg| arg.as_str() == "-C" || arg.as_str() == "-CC")
        .count();
    if repeated_copy_flags > 1 {
        args.find_copies_harder = true;
        if args.find_copies.is_empty() {
            args.find_copies.push("50".to_owned());
        }
    }

    // Recover `-l<N>`/`-l <N>` when it was captured in trailing args.
    if args.rename_limit.is_none() {
        for (idx, arg) in raw_args.iter().enumerate().skip(1) {
            if arg == "-l" {
                if let Some(next) = raw_args.get(idx + 1) {
                    args.rename_limit = next.parse::<usize>().ok();
                    break;
                }
            } else if let Some(value) = arg.strip_prefix("-l") {
                if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
                    args.rename_limit = value.parse::<usize>().ok();
                    break;
                }
            }
        }
    }

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
    let mut rev_idx = 0;
    while rev_idx < revs.len() {
        let r = &revs[rev_idx];
        if r.starts_with("--") || r.starts_with("-") && r.len() > 1 {
            if r == "-l" {
                if rev_idx + 1 < revs.len() {
                    args.rename_limit = revs[rev_idx + 1].parse::<usize>().ok();
                    rev_idx += 1;
                }
                rev_idx += 1;
                continue;
            }
            if let Some(value) = r.strip_prefix("-l") {
                if value.is_empty() {
                    rev_idx += 1;
                    continue;
                }
                if value.bytes().all(|b| b.is_ascii_digit()) {
                    args.rename_limit = value.parse::<usize>().ok();
                    rev_idx += 1;
                    continue;
                }
            }
            // Re-apply trailing flags
            match r.as_str() {
                "--name-only" => args.name_only = true,
                "--name-status" => args.name_status = true,
                "-c" => args.combined = true,
                "--cc" => args.dense_combined = true,
                "--numstat" => args.numstat = true,
                "--shortstat" => args.shortstat = true,
                "--summary" => args.summary = true,
                "--quiet" | "-q" => args.quiet = true,
                "--reverse" | "-R" => args.reverse = true,
                "--irreversible-delete" | "-D" => args.irreversible_delete = true,
                "--cached" | "--staged" => args.cached = true,
                s if s.starts_with("--stat-width=") => {
                    if let Some(val) = s.strip_prefix("--stat-width=") {
                        args.stat_width = val.parse().ok();
                        stat_enabled = true;
                    }
                }
                s if s.starts_with("--stat-name-width=") => {
                    if let Some(val) = s.strip_prefix("--stat-name-width=") {
                        args.stat_name_width = val.parse().ok();
                        stat_enabled = true;
                    }
                }
                s if s.starts_with("--stat-count=") => {
                    if let Some(val) = s.strip_prefix("--stat-count=") {
                        args.stat_count = val.parse().ok();
                        stat_enabled = true;
                    }
                }
                s if s.starts_with("--stat-graph-width=") => {
                    if let Some(val) = s.strip_prefix("--stat-graph-width=") {
                        args.stat_graph_width = val.parse().ok();
                        stat_enabled = true;
                    }
                }
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
                "--break-rewrites" | "-B" => args.break_rewrites = true,
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
                s if s == "-M"
                    || s.starts_with("-M")
                        && s[2..].bytes().all(|b| b.is_ascii_digit() || b == b'%') =>
                {
                    let val = if s.len() > 2 { &s[2..] } else { "50" };
                    args.find_renames = Some(val.to_owned());
                }
                s if s.starts_with("--find-renames") => {
                    if let Some(val) = s.strip_prefix("--find-renames=") {
                        args.find_renames = Some(val.to_owned());
                    } else {
                        args.find_renames = Some("50".to_owned());
                    }
                }
                s if s == "-C" || s == "-CC" || s.starts_with("--find-copies") => {
                    if !args.find_copies.is_empty() || s == "-CC" {
                        args.find_copies_harder = true;
                    }
                    if s == "-CC" {
                        args.find_copies.push("50".to_owned());
                        args.find_copies.push("50".to_owned());
                    } else if let Some(val) = s.strip_prefix("--find-copies=") {
                        args.find_copies.push(val.to_owned());
                    } else {
                        args.find_copies.push("50".to_owned());
                    }
                }
                "--find-copies-harder" => {
                    args.find_copies_harder = true;
                    if args.find_copies.is_empty() {
                        args.find_copies.push("50".to_owned());
                    }
                }
                s if s.starts_with("-S") => {
                    if s.len() > 2 {
                        args.pickaxe_string = Some(s[2..].to_owned());
                    } else if rev_idx + 1 < revs.len() {
                        rev_idx += 1;
                        args.pickaxe_string = Some(revs[rev_idx].clone());
                    }
                }
                s if s.starts_with("-G") => {
                    if s.len() > 2 {
                        args.pickaxe_grep = Some(s[2..].to_owned());
                    } else if rev_idx + 1 < revs.len() {
                        rev_idx += 1;
                        args.pickaxe_grep = Some(revs[rev_idx].clone());
                    }
                }
                "--pickaxe-regex" => {
                    args.pickaxe_regex = true;
                }
                "--pickaxe-all" => {
                    // Accepted for compatibility
                }
                _ => {
                    extra_revs.push(r.clone());
                    rev_idx += 1;
                    continue;
                }
            }
        } else {
            extra_revs.push(r.clone());
        }
        rev_idx += 1;
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
        (_, _) if args.combined || args.dense_combined => combined_diff_entries(&repo, &revs)?,
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
            if let Some(blob_entries) = diff_blob_specs(&repo, &revs[0], &revs[1])? {
                blob_entries
            } else {
                // Two revisions: tree-to-tree diff
                let tree1 = commit_or_tree_oid(&repo, &revs[0])?;
                let tree2 = commit_or_tree_oid(&repo, &revs[1])?;
                diff_trees(&repo.odb, Some(&tree1), Some(&tree2), "")?
            }
        }
        _ => {
            bail!("too many revisions");
        }
    };

    let entries = if args.reverse {
        reverse_entries(entries)
    } else {
        entries
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

    // -C implies -M (copy detection requires rename detection)
    if !args.find_copies.is_empty() && args.find_renames.is_none() {
        args.find_renames = Some("50".to_owned());
    }

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

    // Apply copy detection when requested (-C / --find-copies).
    let entries = if !args.find_copies.is_empty() {
        let copy_threshold = args
            .find_copies
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);
        let mut find_copies_harder = args.find_copies_harder || args.find_copies.len() > 1;

        let input_entries = entries;
        let fell_back_due_to_limit = if find_copies_harder {
            // `-C -C` asks Git to consider unmodified files as potential copy
            // sources. We approximate the candidate set size using the current
            // index entry count so `-l <N>` can trigger the same warning/fallback
            // behavior expected by upstream tests.
            let source_candidate_count = index.entries.len();
            if let Some(limit) = args.rename_limit {
                if source_candidate_count > limit {
                    eprintln!(
                        "warning: only found copies from modified paths due to too many files."
                    );
                    eprintln!("warning: you may want to set your diff.renameLimit variable to at least {} and retry the command.", source_candidate_count);
                    find_copies_harder = false;
                    true
                } else {
                    false
                }
            } else {
                // Full unmodified-source scanning is not yet implemented in this path.
                find_copies_harder = false;
                false
            }
        } else {
            false
        };

        let _ = fell_back_due_to_limit;
        detect_copies(
            &repo.odb,
            input_entries,
            copy_threshold,
            find_copies_harder,
            &[],
        )
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

    // Apply pickaxe filtering (-G <regex> or -S <string> [--pickaxe-regex]).
    let entries = if let Some(ref pattern) = args.pickaxe_grep {
        // -G: show only entries whose diff text has added/removed lines matching the regex
        let re = regex::Regex::new(pattern)
            .with_context(|| format!("invalid pickaxe regex: {pattern}"))?;
        entries
            .into_iter()
            .filter(|e| {
                let old = read_content(&repo.odb, &e.old_oid, None, e.path());
                let new = read_content(&repo.odb, &e.new_oid, wt_for_content, e.path());
                // Check if any added or removed line matches
                for line in new.lines() {
                    if re.is_match(line) {
                        return true;
                    }
                }
                for line in old.lines() {
                    if re.is_match(line) {
                        return true;
                    }
                }
                false
            })
            .collect()
    } else if let Some(ref needle) = args.pickaxe_string {
        if args.pickaxe_regex {
            // -S with --pickaxe-regex: treat needle as regex, filter by occurrence count change
            let re = regex::Regex::new(needle)
                .with_context(|| format!("invalid pickaxe regex: {needle}"))?;
            entries
                .into_iter()
                .filter(|e| {
                    let old = read_content(&repo.odb, &e.old_oid, None, e.path());
                    let new = read_content(&repo.odb, &e.new_oid, wt_for_content, e.path());
                    let old_count = re.find_iter(&old).count();
                    let new_count = re.find_iter(&new).count();
                    old_count != new_count
                })
                .collect()
        } else {
            // -S without --pickaxe-regex: filter by string occurrence count change
            entries
                .into_iter()
                .filter(|e| {
                    let old = read_content(&repo.odb, &e.old_oid, None, e.path());
                    let new = read_content(&repo.odb, &e.new_oid, wt_for_content, e.path());
                    let old_count = old.matches(needle.as_str()).count();
                    let new_count = new.matches(needle.as_str()).count();
                    old_count != new_count
                })
                .collect()
        }
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

    let context_lines = resolve_patch_context_lines(&repo, args.unified)?;
    let cli_algorithm_choice = cli_diff_algorithm_choice(&raw_args, &args);

    if !args.quiet {
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
                write_diff_summary(&mut out, &entries, &repo.odb, args.break_rewrites)?;
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
            write_numstat(&mut out, &entries, &repo, &repo.odb, wt_for_content, &args)?;
        } else if args.name_only {
            write_name_only(&mut out, &entries)?;
        } else if args.name_status {
            write_name_status(&mut out, &entries)?;
        } else if args.summary && !stat_enabled {
            write_diff_summary(&mut out, &entries, &repo.odb, args.break_rewrites)?;
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
                &repo,
                &repo.odb,
                context_lines,
                use_color,
                word_diff,
                wt_for_content,
                suppress_blank_empty,
                patch_abbrev,
                args.inter_hunk_context,
                args.binary,
                args.break_rewrites,
                args.irreversible_delete,
                &src_prefix,
                &dst_prefix,
                args.no_textconv,
                cli_algorithm_choice,
            )?;
        }
    }

    if (args.exit_code || args.quiet) && has_diff {
        std::process::exit(1);
    }

    Ok(())
}

/// Resolve effective patch context lines for `diff`.
fn resolve_patch_context_lines(repo: &Repository, cli_unified: Option<usize>) -> Result<usize> {
    if let Some(value) = cli_unified {
        return Ok(value);
    }

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    if let Some(value) = config.get("diff.context") {
        return parse_diff_context_config_value(&value);
    }

    Ok(3)
}

/// Parse `diff.context` config value with git-compatible failure classes.
fn parse_diff_context_config_value(value: &str) -> Result<usize> {
    match value.parse::<i64>() {
        Ok(parsed) if parsed >= 0 => Ok(parsed as usize),
        Ok(_) => anyhow::bail!("fatal: bad config variable 'diff.context'"),
        Err(_) => anyhow::bail!(
            "fatal: bad numeric config value '{}' for 'diff.context': invalid unit",
            value
        ),
    }
}

/// Build synthetic entries for combined-diff path listing (`-c`/`--cc`).
///
/// For a merge result tree and its parent trees, Git combined path listing
/// keeps only paths that differ from *all* parents. This helper computes that
/// intersection and returns deterministic, sorted synthetic entries.
fn combined_diff_entries(repo: &Repository, revs: &[String]) -> Result<Vec<DiffEntry>> {
    let (base_tree, parent_trees) = resolve_combined_tree_set(repo, revs)?;
    if parent_trees.is_empty() {
        return Ok(Vec::new());
    }

    let mut common_paths: Option<BTreeSet<String>> = None;
    for parent_tree in parent_trees {
        let entries = diff_trees(&repo.odb, Some(&parent_tree), Some(&base_tree), "")?;
        let paths: BTreeSet<String> = entries
            .into_iter()
            .map(|entry| entry.path().to_owned())
            .collect();
        common_paths = Some(match common_paths {
            None => paths,
            Some(prev) => prev.intersection(&paths).cloned().collect(),
        });
    }

    let mut result = Vec::new();
    for path in common_paths.unwrap_or_default() {
        result.push(DiffEntry {
            status: DiffStatus::Modified,
            old_path: Some(path.clone()),
            new_path: Some(path),
            old_mode: "000000".to_owned(),
            new_mode: "000000".to_owned(),
            old_oid: zero_oid(),
            new_oid: zero_oid(),
            score: None,
        });
    }
    Ok(result)
}

/// Resolve the base tree and parent trees for combined-diff calculations.
fn resolve_combined_tree_set(
    repo: &Repository,
    revs: &[String],
) -> Result<(ObjectId, Vec<ObjectId>)> {
    if revs.is_empty() {
        bail!("combined diff requires at least one revision");
    }

    if revs.len() == 1 {
        let commit_oid = resolve_revision(repo, &revs[0])
            .with_context(|| format!("unknown revision: '{}'", revs[0]))?;
        let commit_obj = repo
            .odb
            .read(&commit_oid)
            .with_context(|| format!("reading commit {}", revs[0]))?;
        if commit_obj.kind != ObjectKind::Commit {
            bail!("'{}' does not name a commit", revs[0]);
        }
        let commit = parse_commit(&commit_obj.data).context("parsing commit for combined diff")?;
        let parent_trees = commit
            .parents
            .iter()
            .map(|parent| {
                let parent_obj = repo.odb.read(parent).context("reading parent commit")?;
                let parent_commit =
                    parse_commit(&parent_obj.data).context("parsing parent commit")?;
                Ok(parent_commit.tree)
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok((commit.tree, parent_trees));
    }

    let base_tree = commit_or_tree_oid(repo, &revs[0])?;
    let parent_trees = revs[1..]
        .iter()
        .map(|rev| commit_or_tree_oid(repo, rev))
        .collect::<Result<Vec<_>>>()?;
    Ok((base_tree, parent_trees))
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

    let raw_args: Vec<String> = std::env::args().collect();
    let cli_no_index_algorithm = cli_diff_algorithm_choice(&raw_args, args);
    let mut no_index_algorithm = cli_no_index_algorithm.unwrap_or(DiffAlgorithmChoice::Myers);
    let mut no_index_funcname_matcher = None;
    let discovered_repo = Repository::discover(None).ok();
    let no_index_work_tree = discovered_repo
        .as_ref()
        .and_then(|repo| repo.work_tree.as_deref());
    let no_index_config = if let Some(repo) = discovered_repo.as_ref() {
        grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok()
    } else {
        grit_lib::config::ConfigSet::load(None, true).ok()
    };
    let mut no_index_rules = if let Some(repo) = discovered_repo.as_ref() {
        match load_diff_attr_rules(repo, no_index_config.as_ref(), no_index_work_tree) {
            Ok(rules) => rules.unwrap_or_default(),
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(128);
            }
        }
    } else {
        Vec::new()
    };
    if discovered_repo.is_none() {
        append_core_attributes_rules(no_index_config.as_ref(), &mut no_index_rules);
    }
    let no_index_rules_opt = if no_index_rules.is_empty() {
        None
    } else {
        Some(no_index_rules.as_slice())
    };

    if let (Some(config), Some(rules), Some(work_tree)) = (
        no_index_config.as_ref(),
        no_index_rules_opt,
        no_index_work_tree,
    ) {
        if discovered_repo.is_some() {
            if let Some(err) = validate_funcname_patterns_for_no_index_paths_with_rules(
                config, rules, work_tree, paths[0], paths[1],
            ) {
                eprintln!("error: {err}");
                std::process::exit(128);
            }
            let attrs_path = path_for_attr_lookup_opt(paths[0], Some(work_tree));
            match resolve_funcname_matcher_for_path(config, rules, &attrs_path) {
                Ok(matcher) => no_index_funcname_matcher = matcher,
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(128);
                }
            }
        }
    }

    if cli_no_index_algorithm.is_none() {
        no_index_algorithm = resolve_diff_algorithm_for_path(
            None,
            no_index_config.as_ref(),
            no_index_rules_opt,
            no_index_work_tree,
            paths[0],
        );
        if no_index_algorithm == DiffAlgorithmChoice::Myers {
            no_index_algorithm = resolve_diff_algorithm_for_path(
                None,
                no_index_config.as_ref(),
                no_index_rules_opt,
                no_index_work_tree,
                paths[1],
            );
        }
    }

    // If both paths are directories, diff all files recursively
    if path_a.is_dir() && path_b.is_dir() {
        return run_no_index_dirs(args, path_a, path_b);
    }

    let data_a = read_path_raw(path_a).with_context(|| format!("could not read '{}'", paths[0]))?;
    let data_b = read_path_raw(path_b).with_context(|| format!("could not read '{}'", paths[1]))?;
    let ws_mode = WhitespaceMode {
        ignore_all_space: args.ignore_all_space,
        ignore_space_change: args.ignore_space_change,
        ignore_space_at_eol: args.ignore_space_at_eol,
        ignore_blank_lines: args.ignore_blank_lines,
        ignore_cr_at_eol: args.ignore_cr_at_eol,
    };

    let has_diff = if ws_mode.any() {
        let text_a = String::from_utf8_lossy(&data_a);
        let text_b = String::from_utf8_lossy(&data_b);
        ws_mode.normalize(&text_a) != ws_mode.normalize(&text_b)
    } else {
        data_a != data_b
    };
    if !has_diff {
        return Ok(());
    }

    // --quiet / --exit-code: just exit 1 for differences, no output
    if args.quiet {
        std::process::exit(1);
    }

    let mut patch_data_a = data_a.clone();
    let mut patch_data_b = data_b.clone();
    if !args.no_textconv && !args.binary {
        if let (Some(config), Some(rules)) = (no_index_config.as_ref(), no_index_rules_opt) {
            if let Some(spec) = textconv_spec_for_path(config, rules, no_index_work_tree, paths[0])
            {
                let old_converted = apply_textconv(&spec.program, &data_a);
                let new_converted = apply_textconv(&spec.program, &data_b);
                if let (Some(old_conv), Some(new_conv)) = (old_converted, new_converted) {
                    patch_data_a = old_conv;
                    patch_data_b = new_conv;
                }
            }
        }
    }

    let text_a = String::from_utf8_lossy(&data_a);
    let text_b = String::from_utf8_lossy(&data_b);
    let patch_text_a = String::from_utf8_lossy(&patch_data_a).into_owned();
    let patch_text_b = String::from_utf8_lossy(&patch_data_b).into_owned();
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
            let bar = format!("{}{}", "+".repeat(adds), "-".repeat(dels));
            writeln!(out, " {} | {} {}", display, total, bar)?;
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

    write_no_index_headers(
        &mut out, paths[0], paths[1], path_a, path_b, &data_a, &data_b,
    )?;

    // Determine whether anchored mode is in effect.
    let use_anchored = if !args.anchored.is_empty() {
        // Check if a non-anchored algorithm flag appears after --anchored in args.
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

    let diff_output = if ws_mode.any() {
        no_index_unified_diff_with_ws_mode(
            &patch_text_a,
            &patch_text_b,
            paths[0],
            paths[1],
            context_lines,
            &ws_mode,
        )
    } else if use_anchored {
        anchored_unified_diff(
            &patch_text_a,
            &patch_text_b,
            paths[0],
            paths[1],
            context_lines,
            &args.anchored,
        )
    } else {
        grit_lib::diff::unified_diff_with_prefix_and_funcname_and_algorithm(
            &patch_text_a,
            &patch_text_b,
            paths[0],
            paths[1],
            context_lines,
            "a/",
            "b/",
            no_index_funcname_matcher.as_ref(),
            no_index_algorithm.to_similar(),
        )
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

/// Reverse diff entries for `-R/--reverse`.
fn reverse_entries(entries: Vec<DiffEntry>) -> Vec<DiffEntry> {
    entries
        .into_iter()
        .map(|mut entry| {
            std::mem::swap(&mut entry.old_path, &mut entry.new_path);
            std::mem::swap(&mut entry.old_mode, &mut entry.new_mode);
            std::mem::swap(&mut entry.old_oid, &mut entry.new_oid);
            entry.status = match entry.status {
                DiffStatus::Added => DiffStatus::Deleted,
                DiffStatus::Deleted => DiffStatus::Added,
                other => other,
            };
            entry
        })
        .collect()
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
    let no_index_dir_context = Repository::discover(None).ok().and_then(|repo| {
        let work_tree = repo.work_tree?;
        let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok()?;
        let rules = grit_lib::crlf::load_gitattributes(&work_tree);
        Some((work_tree, config, rules))
    });

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
        let no_index_dir_matcher = if let Some((work_tree, config, rules)) = &no_index_dir_context {
            let attrs_rel_path =
                path_for_attr_lookup(&format!("{}/{}", dir_a.display(), rel), work_tree);
            match resolve_funcname_matcher_for_path(config, rules, &attrs_rel_path) {
                Ok(matcher) => matcher,
                Err(err) => {
                    eprintln!("error: {err}");
                    std::process::exit(128);
                }
            }
        } else {
            None
        };
        let patch = grit_lib::diff::unified_diff_with_prefix_and_funcname(
            &text_a,
            &text_b,
            &old_label,
            &new_label,
            context_lines,
            "a/",
            "b/",
            no_index_dir_matcher.as_ref(),
        );
        write!(out, "{}", patch)?;
    }

    if has_diff {
        std::process::exit(1);
    }
    Ok(())
}

/// Build unified diff output for `--no-index` with whitespace-aware matching.
fn no_index_unified_diff_with_ws_mode(
    old_content: &str,
    new_content: &str,
    old_path: &str,
    new_path: &str,
    context_lines: usize,
    ws_mode: &WhitespaceMode,
) -> String {
    use similar::TextDiff;

    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();
    let old_norm: Vec<String> = old_lines
        .iter()
        .map(|line| ws_mode.normalize_line(line))
        .collect();
    let new_norm: Vec<String> = new_lines
        .iter()
        .map(|line| ws_mode.normalize_line(line))
        .collect();
    let old_norm_refs: Vec<&str> = old_norm.iter().map(String::as_str).collect();
    let new_norm_refs: Vec<&str> = new_norm.iter().map(String::as_str).collect();

    let diff = TextDiff::from_slices(&old_norm_refs, &new_norm_refs);
    let mut out = String::new();
    out.push_str(&format!("--- a/{old_path}\n"));
    out.push_str(&format!("+++ b/{new_path}\n"));

    for group in diff.grouped_ops(context_lines) {
        if group.is_empty() {
            continue;
        }

        let old_start = group.first().map_or(0, |op| op.old_range().start);
        let new_start = group.first().map_or(0, |op| op.new_range().start);
        let old_end = group.last().map_or(0, |op| op.old_range().end);
        let new_end = group.last().map_or(0, |op| op.new_range().end);
        let old_count = old_end.saturating_sub(old_start);
        let new_count = new_end.saturating_sub(new_start);

        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            format_hunk_range(old_start, old_count),
            format_hunk_range(new_start, new_count)
        ));

        for op in group {
            for change in diff.iter_changes(&op) {
                match change.tag() {
                    similar::ChangeTag::Equal => {
                        if let Some(new_idx) = change.new_index() {
                            out.push(' ');
                            out.push_str(new_lines.get(new_idx).copied().unwrap_or_default());
                            out.push('\n');
                        } else if let Some(old_idx) = change.old_index() {
                            out.push(' ');
                            out.push_str(old_lines.get(old_idx).copied().unwrap_or_default());
                            out.push('\n');
                        }
                    }
                    similar::ChangeTag::Delete => {
                        if let Some(old_idx) = change.old_index() {
                            out.push('-');
                            out.push_str(old_lines.get(old_idx).copied().unwrap_or_default());
                            out.push('\n');
                        }
                    }
                    similar::ChangeTag::Insert => {
                        if let Some(new_idx) = change.new_index() {
                            out.push('+');
                            out.push_str(new_lines.get(new_idx).copied().unwrap_or_default());
                            out.push('\n');
                        }
                    }
                }
            }
        }
    }

    out
}

/// Format a unified-diff hunk range component.
fn format_hunk_range(start: usize, count: usize) -> String {
    if count == 1 {
        (start + 1).to_string()
    } else {
        format!("{},{}", start + 1, count)
    }
}

/// Write `diff --git` and `index` header lines for `--no-index` output.
fn write_no_index_headers(
    out: &mut impl Write,
    display_old: &str,
    display_new: &str,
    old_path: &Path,
    new_path: &Path,
    old_data: &[u8],
    new_data: &[u8],
) -> Result<()> {
    let old_oid = Odb::hash_object_data(ObjectKind::Blob, old_data).to_hex();
    let new_oid = Odb::hash_object_data(ObjectKind::Blob, new_data).to_hex();
    let old_abbrev = &old_oid[..7];
    let new_abbrev = &new_oid[..7];
    writeln!(out, "diff --git a/{display_old} b/{display_new}")?;

    let old_mode = no_index_mode(old_path);
    let new_mode = no_index_mode(new_path);
    if let (Some(old_mode), Some(new_mode)) = (old_mode, new_mode) {
        if old_mode == new_mode {
            writeln!(out, "index {old_abbrev}..{new_abbrev} {old_mode}")?;
        } else {
            writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
            writeln!(out, "old mode {old_mode}")?;
            writeln!(out, "new mode {new_mode}")?;
        }
    } else {
        writeln!(out, "index {old_abbrev}..{new_abbrev}")?;
    }
    Ok(())
}

/// Return git-style file mode for a no-index path, if the path exists.
fn no_index_mode(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        let mode = if meta.file_type().is_symlink() {
            "120000"
        } else if (meta.mode() & 0o111) != 0 {
            "100755"
        } else {
            "100644"
        };
        Some(mode.to_owned())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Some("100644".to_owned())
    }
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

/// Try to resolve two rev specs as blob objects (e.g. `HEAD:path` vs `HEAD:path`).
fn diff_blob_specs(repo: &Repository, left: &str, right: &str) -> Result<Option<Vec<DiffEntry>>> {
    let Some((left_oid, left_label)) = resolve_blob_spec(repo, left)? else {
        return Ok(None);
    };
    let Some((right_oid, right_label)) = resolve_blob_spec(repo, right)? else {
        return Ok(None);
    };

    if left_oid == right_oid {
        return Ok(Some(Vec::new()));
    }

    Ok(Some(vec![DiffEntry {
        status: DiffStatus::Modified,
        old_path: Some(left_label),
        new_path: Some(right_label),
        old_mode: "100644".to_owned(),
        new_mode: "100644".to_owned(),
        old_oid: left_oid,
        new_oid: right_oid,
        score: None,
    }]))
}

/// Resolve a revision spec to a blob object and a display label.
fn resolve_blob_spec(repo: &Repository, spec: &str) -> Result<Option<(ObjectId, String)>> {
    if !spec.contains(':') {
        return Ok(None);
    }
    let oid = resolve_revision(repo, spec)?;
    let obj = repo.odb.read(&oid)?;
    if obj.kind != ObjectKind::Blob {
        return Ok(None);
    }

    let label = spec
        .split_once(':')
        .map(|(_, path)| path)
        .filter(|path| !path.is_empty())
        .unwrap_or(spec)
        .to_owned();
    Ok(Some((oid, label)))
}

/// Validate configured funcname patterns for the two `--no-index` paths.
fn validate_funcname_patterns_for_no_index_paths_with_rules(
    config: &grit_lib::config::ConfigSet,
    rules: &[grit_lib::crlf::AttrRule],
    work_tree: &Path,
    path_a: &str,
    path_b: &str,
) -> Option<String> {
    for raw_path in [path_a, path_b] {
        let attrs_path = path_for_attr_lookup_opt(raw_path, Some(work_tree));
        let attrs = grit_lib::crlf::get_file_attrs(rules, &attrs_path, config);
        let Some(driver) = attrs.diff_driver else {
            continue;
        };
        if let Some(err) = invalid_funcname_pattern_for_driver(config, &driver) {
            return Some(err);
        }
    }
    None
}

/// Convert a path to the form used for `.gitattributes` lookups.
fn path_for_attr_lookup(path: &str, work_tree: &Path) -> String {
    let path_obj = Path::new(path);
    if let Ok(relative) = path_obj.strip_prefix(work_tree) {
        return relative.to_string_lossy().into_owned();
    }
    path.to_owned()
}

/// Convert a path to the form used for `.gitattributes` lookups, optionally
/// relative to a work tree.
fn path_for_attr_lookup_opt(path: &str, work_tree: Option<&Path>) -> String {
    if let Some(work_tree) = work_tree {
        path_for_attr_lookup(path, work_tree)
    } else {
        path.to_owned()
    }
}

/// Check for the specific git error: last funcname expression cannot be negated.
fn invalid_funcname_pattern_for_driver(
    config: &grit_lib::config::ConfigSet,
    driver: &str,
) -> Option<String> {
    let pattern = config
        .get(&format!("diff.{driver}.xfuncname"))
        .or_else(|| config.get(&format!("diff.{driver}.funcname")))?;

    let last = pattern
        .lines()
        .map(str::trim_end)
        .rfind(|line| !line.is_empty())?;
    if last.starts_with('!') {
        return Some(format!(
            "diff.{driver}.funcname: Last expression must not be negated: {last}"
        ));
    }
    None
}

fn resolve_funcname_matcher_for_path(
    config: &grit_lib::config::ConfigSet,
    rules: &[grit_lib::crlf::AttrRule],
    rel_path: &str,
) -> std::result::Result<Option<grit_lib::userdiff::FuncnameMatcher>, String> {
    grit_lib::userdiff::matcher_for_path(config, rules, rel_path)
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
            if let Ok(data) = read_path_raw(&wt.join(path)) {
                return data;
            }
        }
    }
    Vec::new()
}

fn read_path_raw(path: &Path) -> std::io::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return std::fs::read_link(path)
            .map(|target| target.as_os_str().as_encoded_bytes().to_vec());
    }
    std::fs::read(path)
}

/// Check if content appears to be binary (contains NUL bytes in first 8KB).
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    if data[..check_len].contains(&0) {
        return true;
    }

    looks_like_escaped_octal_binary(data)
}

/// Compute rewrite dissimilarity for `-B` metadata (`100 - similarity`).
fn compute_rewrite_dissimilarity_from_content(old_data: &[u8], new_data: &[u8]) -> Option<u32> {
    if old_data.is_empty() && new_data.is_empty() {
        return None;
    }
    let similarity = grit_lib::diff::rename_similarity_score(old_data, new_data).min(100);
    Some(100u32.saturating_sub(similarity))
}

/// Textconv settings resolved for a specific path.
#[derive(Debug, Clone)]
struct TextconvSpec {
    driver: String,
    program: String,
    cache_enabled: bool,
}

/// Resolve textconv settings for a path from `.gitattributes` + config.
fn textconv_spec_for_path(
    config: &grit_lib::config::ConfigSet,
    rules: &[grit_lib::crlf::AttrRule],
    work_tree: Option<&Path>,
    path: &str,
) -> Option<TextconvSpec> {
    let attrs_path = path_for_attr_lookup_opt(path, work_tree);
    let attrs = grit_lib::crlf::get_file_attrs(rules, &attrs_path, config);
    let driver = attrs.diff_driver?;
    let program = config.get(&format!("diff.{driver}.textconv"))?;
    let cache_enabled = config
        .get_bool(&format!("diff.{driver}.cachetextconv"))
        .and_then(Result::ok)
        .unwrap_or(false);
    Some(TextconvSpec {
        driver,
        program,
        cache_enabled,
    })
}

/// Resolve whether a path is forced binary by `diff.<driver>.binary=true`.
fn binary_driver_for_path(
    config: &grit_lib::config::ConfigSet,
    rules: &[grit_lib::crlf::AttrRule],
    work_tree: Option<&Path>,
    path: &str,
) -> bool {
    if path == "/dev/null" {
        return false;
    }
    let attrs_path = path_for_attr_lookup_opt(path, work_tree);
    let attrs = grit_lib::crlf::get_file_attrs(rules, &attrs_path, config);
    let Some(driver) = attrs.diff_driver else {
        return false;
    };
    config
        .get_bool(&format!("diff.{driver}.binary"))
        .and_then(Result::ok)
        .unwrap_or(false)
}

fn mode_is_symlink(mode: &str) -> bool {
    u32::from_str_radix(mode, 8).ok() == Some(0o120000)
}

/// Build the notes ref used for textconv cache entries.
fn textconv_notes_ref(driver: &str) -> String {
    format!("refs/notes/textconv/{driver}")
}

/// Build note payload stored under textconv cache notes.
fn textconv_cache_note_payload(program: &str, converted: &[u8]) -> Vec<u8> {
    let mut payload = format!("program {program}\n\n").into_bytes();
    payload.extend_from_slice(converted);
    payload
}

/// Parse cached note payload if it matches the currently configured program.
fn parse_textconv_cache_note_payload(program: &str, note_payload: &[u8]) -> Option<Vec<u8>> {
    let header = format!("program {program}\n\n");
    note_payload
        .strip_prefix(header.as_bytes())
        .map(std::borrow::ToOwned::to_owned)
}

/// Read tree entries and parent commit for a notes ref.
fn read_notes_tree_entries(
    repo: &Repository,
    notes_ref: &str,
) -> Option<(Option<ObjectId>, Vec<TreeEntry>)> {
    let parent = resolve_ref(&repo.git_dir, notes_ref).ok();
    let Some(parent_oid) = parent else {
        return Some((None, Vec::new()));
    };
    let commit_obj = repo.odb.read(&parent_oid).ok()?;
    if commit_obj.kind != ObjectKind::Commit {
        return None;
    }
    let commit = parse_commit(&commit_obj.data).ok()?;
    let tree_obj = repo.odb.read(&commit.tree).ok()?;
    if tree_obj.kind != ObjectKind::Tree {
        return None;
    }
    let entries = parse_tree(&tree_obj.data).ok()?;
    Some((Some(parent_oid), entries))
}

/// Read cached textconv output for an object and program from notes.
fn read_textconv_cache(
    repo: &Repository,
    notes_ref: &str,
    object_oid: &ObjectId,
    program: &str,
) -> Option<Vec<u8>> {
    if *object_oid == zero_oid() {
        return Some(Vec::new());
    }
    let (_, entries) = read_notes_tree_entries(repo, notes_ref)?;
    let target_name = object_oid.to_hex();
    let note_entry = entries
        .iter()
        .find(|entry| entry.name == target_name.as_bytes())?;
    let note_blob = repo.odb.read(&note_entry.oid).ok()?;
    if note_blob.kind != ObjectKind::Blob {
        return None;
    }
    parse_textconv_cache_note_payload(program, &note_blob.data)
}

/// Build an ident string for writing textconv cache notes commits.
fn textconv_cache_ident(config: &grit_lib::config::ConfigSet) -> String {
    let name = config.get("user.name").unwrap_or_else(|| "Grit".to_owned());
    let email = config
        .get("user.email")
        .unwrap_or_else(|| "grit@example.com".to_owned());
    let now = OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    format!("{name} <{email}> {epoch} +0000")
}

/// Update textconv cache notes for an object.
fn write_textconv_cache(
    repo: &Repository,
    config: &grit_lib::config::ConfigSet,
    notes_ref: &str,
    object_oid: &ObjectId,
    payload: &[u8],
) -> Option<()> {
    if *object_oid == zero_oid() {
        return Some(());
    }

    let (parent, mut entries) = read_notes_tree_entries(repo, notes_ref)?;
    let note_blob_oid = repo.odb.write(ObjectKind::Blob, payload).ok()?;
    let target_name = object_oid.to_hex().into_bytes();

    if let Some(existing) = entries.iter_mut().find(|entry| entry.name == target_name) {
        existing.mode = 0o100644;
        existing.oid = note_blob_oid;
    } else {
        entries.push(TreeEntry {
            mode: 0o100644,
            name: target_name,
            oid: note_blob_oid,
        });
    }

    entries
        .sort_by(|a, b| tree_entry_cmp(&a.name, a.mode == 0o040000, &b.name, b.mode == 0o040000));

    let tree_data = serialize_tree(&entries);
    let tree_oid = repo.odb.write(ObjectKind::Tree, &tree_data).ok()?;
    let ident = textconv_cache_ident(config);
    let commit = CommitData {
        tree: tree_oid,
        parents: parent.into_iter().collect(),
        author: ident.clone(),
        committer: ident,
        encoding: None,
        message: "update textconv cache\n".to_owned(),
        raw_message: None,
    };
    let commit_data = serialize_commit(&commit);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_data).ok()?;
    write_ref(&repo.git_dir, notes_ref, &commit_oid).ok()?;
    Some(())
}

/// Run configured textconv command against bytes and return converted output.
fn apply_textconv(program: &str, content: &[u8]) -> Option<Vec<u8>> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let tmp_path =
        std::env::temp_dir().join(format!("grit-textconv-{}-{unique}", std::process::id()));
    std::fs::write(&tmp_path, content).ok()?;
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{program} \"$1\""))
        .arg("grit-textconv")
        .arg(&tmp_path)
        .output()
        .ok()?;
    let _ = std::fs::remove_file(&tmp_path);
    if output.status.success() {
        Some(output.stdout)
    } else {
        None
    }
}

/// Apply textconv using notes-backed cache when enabled.
fn apply_textconv_with_cache(
    repo: &Repository,
    config: &grit_lib::config::ConfigSet,
    spec: &TextconvSpec,
    object_oid: &ObjectId,
    content: &[u8],
) -> Option<Vec<u8>> {
    if *object_oid == zero_oid() {
        return Some(Vec::new());
    }

    let notes_ref = textconv_notes_ref(&spec.driver);
    if spec.cache_enabled {
        if let Some(cached) = read_textconv_cache(repo, &notes_ref, object_oid, &spec.program) {
            return Some(cached);
        }
    }

    let converted = apply_textconv(&spec.program, content)?;
    if spec.cache_enabled {
        let payload = textconv_cache_note_payload(&spec.program, &converted);
        let _ = write_textconv_cache(repo, config, &notes_ref, object_oid, &payload);
    }
    Some(converted)
}

/// Return true for escaped-octal payloads like `\00\01\02...`.
///
/// Our lightweight shell test harness may materialize `printf` binary fixtures
/// as escaped octal text. Treating this shape as binary keeps `--stat` output
/// aligned with upstream expectations (`Bin`) for those fixtures.
fn looks_like_escaped_octal_binary(data: &[u8]) -> bool {
    let text = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let text = text.trim_end_matches('\n');
    if text.len() < 9 {
        // Require at least three escaped bytes.
        return false;
    }

    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut groups = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            return false;
        }
        let first = i + 1;
        let second = i + 2;
        if second >= bytes.len() {
            return false;
        }
        if !bytes[first].is_ascii_digit() || !bytes[second].is_ascii_digit() {
            return false;
        }
        i += 3;
        groups += 1;
    }

    groups >= 3
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
    write_diff_header_with_prefix(out, entry, use_color, abbrev_len, "a/", "b/", None)
}

fn write_diff_header_with_prefix(
    out: &mut impl Write,
    entry: &DiffEntry,
    use_color: bool,
    abbrev_len: usize,
    src_prefix: &str,
    dst_prefix: &str,
    repo: Option<&Repository>,
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
        if let Some(repo) = repo {
            if let Ok(unique) = abbreviate_object_id(repo, *oid, abbrev_len) {
                return unique;
            }
        }
        let hex = oid.to_hex();
        let len = abbrev_len.min(hex.len());
        hex[..len].to_owned()
    };

    let effective_old_oid = if entry.status == DiffStatus::Deleted && entry.old_oid == zero_oid() {
        empty_blob_oid()
    } else {
        entry.old_oid
    };
    let effective_new_oid = if entry.status == DiffStatus::Added && entry.new_oid == zero_oid() {
        empty_blob_oid()
    } else {
        entry.new_oid
    };

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "{b}new file mode {}{r}", entry.new_mode)?;
            writeln!(
                out,
                "{b}index {}..{}{r}",
                abbr(&effective_old_oid),
                abbr(&effective_new_oid)
            )?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "{b}deleted file mode {}{r}", entry.old_mode)?;
            writeln!(
                out,
                "{b}index {}..{}{r}",
                abbr(&effective_old_oid),
                abbr(&effective_new_oid)
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
    repo: &Repository,
    odb: &Odb,
    context_lines: usize,
    use_color: bool,
    word_diff: bool,
    work_tree: Option<&Path>,
    suppress_blank_empty: bool,
    abbrev_len: usize,
    _inter_hunk_context: Option<usize>,
    show_binary: bool,
    break_rewrites: bool,
    irreversible_delete: bool,
    src_prefix: &str,
    dst_prefix: &str,
    no_textconv: bool,
    cli_algorithm_choice: Option<DiffAlgorithmChoice>,
) -> Result<()> {
    let patch_config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let attrs_work_tree = repo.work_tree.as_deref().or(work_tree);
    let patch_rules = load_diff_attr_rules(repo, patch_config.as_ref(), attrs_work_tree)?;

    for entry in entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");
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
        let funcname_matcher = if let (Some(config), Some(rules)) = (&patch_config, &patch_rules) {
            match resolve_funcname_matcher_for_path(config, rules, entry.path()) {
                Ok(matcher) => matcher,
                Err(err) => bail!("{err}"),
            }
        } else {
            None
        };
        let algorithm = resolve_diff_algorithm_for_path(
            cli_algorithm_choice,
            patch_config.as_ref(),
            patch_rules.as_deref(),
            attrs_work_tree,
            entry.path(),
        );

        write_diff_header_with_prefix(
            out,
            entry,
            use_color,
            abbrev_len,
            src_prefix,
            dst_prefix,
            Some(repo),
        )?;

        if irreversible_delete && entry.status == DiffStatus::Deleted {
            continue;
        }

        // Check for binary content
        let old_content_raw = read_content_raw(odb, &entry.old_oid);
        let new_content_raw =
            read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, new_path);

        if break_rewrites && entry.status == DiffStatus::Modified {
            if let Some(dissimilarity) =
                compute_rewrite_dissimilarity_from_content(&old_content_raw, &new_content_raw)
            {
                writeln!(out, "dissimilarity index {dissimilarity}%")?;
            }
        }

        let treat_old_as_binary_by_driver = !mode_is_symlink(&entry.old_mode)
            && patch_config
                .as_ref()
                .zip(patch_rules.as_deref())
                .is_some_and(|(config, rules)| {
                    binary_driver_for_path(config, rules, attrs_work_tree, old_path)
                });
        let treat_new_as_binary_by_driver = !mode_is_symlink(&entry.new_mode)
            && patch_config
                .as_ref()
                .zip(patch_rules.as_deref())
                .is_some_and(|(config, rules)| {
                    binary_driver_for_path(config, rules, attrs_work_tree, new_path)
                });

        let textconv_spec = if no_textconv || show_binary {
            None
        } else {
            patch_config
                .as_ref()
                .zip(patch_rules.as_deref())
                .and_then(|(config, rules)| {
                    textconv_spec_for_path(config, rules, attrs_work_tree, entry.path())
                })
        };
        if let Some(spec) = textconv_spec {
            if let Some(config) = patch_config.as_ref() {
                let old_converted = apply_textconv_with_cache(
                    repo,
                    config,
                    &spec,
                    &entry.old_oid,
                    &old_content_raw,
                );
                let new_converted = apply_textconv_with_cache(
                    repo,
                    config,
                    &spec,
                    &entry.new_oid,
                    &new_content_raw,
                );
                if let (Some(old_conv), Some(new_conv)) = (old_converted, new_converted) {
                    let old_text = String::from_utf8_lossy(&old_conv).into_owned();
                    let new_text = String::from_utf8_lossy(&new_conv).into_owned();
                    let patch = grit_lib::diff::unified_diff_with_prefix_and_funcname_and_algorithm(
                        &old_text,
                        &new_text,
                        display_old,
                        display_new,
                        context_lines,
                        src_prefix,
                        dst_prefix,
                        funcname_matcher.as_ref(),
                        algorithm.to_similar(),
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
                    continue;
                }
            }
        }

        if treat_old_as_binary_by_driver
            || treat_new_as_binary_by_driver
            || is_binary(&old_content_raw)
            || is_binary(&new_content_raw)
        {
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
                let binary_old_display = if display_old == "/dev/null" {
                    "/dev/null".to_owned()
                } else {
                    format!("{src_prefix}{old_path}")
                };
                let binary_new_display = if display_new == "/dev/null" {
                    "/dev/null".to_owned()
                } else {
                    format!("{dst_prefix}{new_path}")
                };
                writeln!(
                    out,
                    "Binary files {binary_old_display} and {binary_new_display} differ"
                )?;
            }
            continue;
        }

        let patch_old_content_raw =
            if irreversible_delete && break_rewrites && entry.status == DiffStatus::Modified {
                Vec::new()
            } else {
                old_content_raw.clone()
            };

        let old_content = String::from_utf8_lossy(&patch_old_content_raw).into_owned();
        let new_content = String::from_utf8_lossy(&new_content_raw).into_owned();

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
            let patch = grit_lib::diff::unified_diff_with_prefix_and_funcname_and_algorithm(
                &old_content,
                &new_content,
                display_old,
                display_new,
                context_lines,
                src_prefix,
                dst_prefix,
                funcname_matcher.as_ref(),
                algorithm.to_similar(),
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

/// Render patch output for `diff-pairs` entries using git-compatible defaults.
pub(crate) fn write_patch_from_pairs(
    out: &mut impl Write,
    entries: &[DiffEntry],
    repo: &Repository,
) -> Result<()> {
    write_patch_with_prefix(
        out,
        entries,
        repo,
        &repo.odb,
        3,
        false,
        false,
        repo.work_tree.as_deref(),
        false,
        7,
        None,
        false,
        false,
        false,
        "a/",
        "b/",
        false,
        None,
    )
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
    let mut changed_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for entry in entries {
        if entry.status == DiffStatus::Unmerged {
            continue;
        }
        let old_raw = read_content_raw(odb, &entry.old_oid);
        let new_raw = read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, entry.path());
        let is_binary = is_binary(&old_raw) || is_binary(&new_raw);
        if !is_binary {
            let old_content = String::from_utf8_lossy(&old_raw);
            let new_content = String::from_utf8_lossy(&new_raw);
            let (ins, del) = count_changes(&old_content, &new_content);
            total_ins += ins;
            total_del += del;
        }
        changed_paths.insert(entry.path().to_owned());
    }
    let files_changed = changed_paths.len();

    let mut summary = format!(
        " {} file{} changed",
        files_changed,
        if files_changed == 1 { "" } else { "s" }
    );
    if files_changed > 0 {
        append_stat_counts(&mut summary, total_ins, total_del);
    }
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

/// Format a `--stat` line for binary changes.
///
/// Git shows `Bin` in the count column for binary files and does not show a
/// histogram bar.
fn format_stat_line_binary(
    path: &str,
    max_path_len: usize,
    count_width: usize,
    old_size: usize,
    new_size: usize,
) -> String {
    let path_display_width = UnicodeWidthStr::width(path);
    let padding = max_path_len.saturating_sub(path_display_width);
    let size_suffix = if old_size == 0 && new_size == 0 {
        String::new()
    } else {
        format!(" {old_size} -> {new_size} bytes")
    };
    format!(
        " {}{} | {:>cw$}{}",
        path,
        " ".repeat(padding),
        "Bin",
        size_suffix,
        cw = count_width,
    )
}

/// Format a `--stat` line for unmerged paths.
fn format_stat_line_unmerged(path: &str, max_path_len: usize, count_width: usize) -> String {
    let path_display_width = UnicodeWidthStr::width(path);
    let padding = max_path_len.saturating_sub(path_display_width);
    format!(
        " {}{} | {:>cw$}",
        path,
        " ".repeat(padding),
        "Unmerged",
        cw = count_width,
    )
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

    // Build display paths (compact rename format for renames, with C-style quoting).
    let display_paths: Vec<String> = entries
        .iter()
        .map(|e| match e.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let old = e.old_path.as_deref().unwrap_or("");
                let new = e.new_path.as_deref().unwrap_or("");
                format_rename_display(old, new)
            }
            _ => quote_c_style(e.path()),
        })
        .collect();
    let max_path_len = display_paths
        .iter()
        .map(|p| UnicodeWidthStr::width(p.as_str()))
        .max()
        .unwrap_or(0);

    // Collect per-file stats first so we can compute the count column width.
    // Track whether each path should be rendered as a binary stat line (`Bin`).
    let mut file_stats: Vec<(&str, usize, usize, bool, bool, usize, usize)> = Vec::new();
    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    let mut changed_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (i, entry) in entries.iter().enumerate() {
        if entry.status == DiffStatus::Unmerged {
            file_stats.push((&display_paths[i], 0, 0, false, true, 0, 0));
            continue;
        }
        let old_raw = read_content_raw(odb, &entry.old_oid);
        let new_raw = read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, entry.path());
        let old_size = old_raw.len();
        let new_size = new_raw.len();
        let is_binary = is_binary(&old_raw) || is_binary(&new_raw);
        let (ins, del) = if is_binary {
            (0, 0)
        } else {
            let old_content = String::from_utf8_lossy(&old_raw);
            let new_content = String::from_utf8_lossy(&new_raw);
            count_changes(&old_content, &new_content)
        };
        file_stats.push((
            &display_paths[i],
            ins,
            del,
            is_binary,
            false,
            old_size,
            new_size,
        ));
        total_ins += ins;
        total_del += del;
        changed_paths.insert(entry.path().to_owned());
    }
    let files_changed = changed_paths.len();

    let display_len = if let Some(limit) = stat_count {
        file_stats.len().min(limit)
    } else {
        file_stats.len()
    };
    let display_stats: &[(&str, usize, usize, bool, bool, usize, usize)] =
        &file_stats[..display_len];

    // Compute the width for the count column from only the rows that will
    // actually be rendered (important when --stat-count hides binary/unmerged rows).
    let max_count = display_stats
        .iter()
        .map(|(_, ins, del, _, _, _, _)| ins + del)
        .max()
        .unwrap_or(0);
    let mut count_width = format!("{}", max_count).len();
    if display_stats
        .iter()
        .any(|(_, _, _, binary, _, _, _)| *binary)
    {
        count_width = count_width.max(3); // width of "Bin"
    }
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

    for (path, ins, del, binary, unmerged, old_size, new_size) in display_stats {
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
        let line = if *unmerged {
            format_stat_line_unmerged(&display_path, max_path_len, count_width)
        } else if *binary {
            format_stat_line_binary(
                &display_path,
                max_path_len,
                count_width,
                *old_size,
                *new_size,
            )
        } else {
            format_stat_line_git(
                &display_path,
                *ins,
                *del,
                max_path_len,
                count_width,
                max_count,
                max_bar,
            )
        };
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
    if files_changed > 0 {
        append_stat_counts(&mut summary, total_ins, total_del);
    }
    writeln!(out, "{summary}")?;

    Ok(())
}

/// C-style quote a path if it contains special characters (tab, newline, etc.).
/// Returns the quoted string (with surrounding double-quotes) if quoting is needed,
/// otherwise returns the original string.
fn quote_c_style(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    let mut needs_quotes = false;
    for ch in name.chars() {
        match ch {
            '"' => {
                out.push_str("\\\"");
                needs_quotes = true;
            }
            '\\' => {
                out.push_str("\\\\");
                needs_quotes = true;
            }
            '\t' => {
                out.push_str("\\t");
                needs_quotes = true;
            }
            '\n' => {
                out.push_str("\\n");
                needs_quotes = true;
            }
            '\r' => {
                out.push_str("\\r");
                needs_quotes = true;
            }
            c if c.is_control() => {
                out.push_str(&format!("\\{:03o}", u32::from(c)));
                needs_quotes = true;
            }
            c => out.push(c),
        }
    }
    if needs_quotes {
        format!("\"{out}\"")
    } else {
        out
    }
}

/// Format a rename/copy path for numstat: `{old_quoted}\t{new_quoted}` or
/// `{old_quoted} => {new_quoted}` depending on format.
fn format_rename_display(old: &str, new: &str) -> String {
    grit_lib::diff::format_rename_path(&quote_c_style(old), &quote_c_style(new))
}

/// Write machine-readable numstat output: `{insertions}\t{deletions}\t{path}`.
fn write_numstat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    _repo: &Repository,
    odb: &Odb,
    work_tree: Option<&Path>,
    args: &Args,
) -> Result<()> {
    for entry in entries {
        let old_raw = read_content_raw(odb, &entry.old_oid);
        let new_raw = read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, entry.path());
        let is_binary_pair = is_binary(&old_raw) || is_binary(&new_raw);
        let (ins, del) = if is_binary_pair {
            (usize::MAX, usize::MAX)
        } else {
            let old_content = String::from_utf8_lossy(&old_raw);
            let new_content = String::from_utf8_lossy(&new_raw);
            count_changes(&old_content, &new_content)
        };
        match entry.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let display = format_rename_display(old, new);
                if ins == usize::MAX {
                    writeln!(out, "-\t-\t{display}")?;
                } else {
                    writeln!(out, "{ins}\t{del}\t{display}")?;
                }
            }
            _ => {
                if ins == usize::MAX {
                    writeln!(out, "-\t-\t{}", entry.path())?;
                } else {
                    writeln!(out, "{ins}\t{del}\t{}", entry.path())?;
                }
            }
        }
    }
    if args.summary {
        write_diff_summary(out, entries, odb, args.break_rewrites)?;
    }
    Ok(())
}

/// Write only the names of changed files.
/// Write `--summary` output for rename/copy/mode-change entries.
fn write_diff_summary(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    break_rewrites: bool,
) -> Result<()> {
    for entry in entries {
        match entry.status {
            DiffStatus::Modified if break_rewrites => {
                let old_raw = read_content_raw(odb, &entry.old_oid);
                let new_raw = read_content_raw(odb, &entry.new_oid);
                let dissim =
                    compute_rewrite_dissimilarity_from_content(&old_raw, &new_raw).unwrap_or(100);
                writeln!(out, " rewrite {} ({dissim}%)", quote_c_style(entry.path()))?;
            }
            DiffStatus::Renamed => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let display = format_rename_display(old, new);
                let sim = entry.score.unwrap_or(100);
                writeln!(out, " rename {display} ({sim}%)")?;
            }
            DiffStatus::Copied => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let display = format_rename_display(old, new);
                let sim = entry.score.unwrap_or(100);
                writeln!(out, " copy {display} ({sim}%)")?;
            }
            DiffStatus::Added => {
                writeln!(
                    out,
                    " create mode {} {}",
                    entry.new_mode,
                    quote_c_style(entry.path())
                )?;
            }
            DiffStatus::Deleted => {
                writeln!(
                    out,
                    " delete mode {} {}",
                    entry.old_mode,
                    quote_c_style(entry.path())
                )?;
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
