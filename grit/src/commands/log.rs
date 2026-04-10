//! `grit log` — show commit logs.
//!
//! Displays the commit history starting from HEAD (or specified revisions),
//! with configurable formatting and filtering.

use crate::explicit_exit::ExplicitExit;
use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::combined_tree_diff::{combined_diff_paths_filtered, CombinedTreeDiffOptions};
use grit_lib::commit_graph_file::{
    BloomPrecheck, BloomWalkStats, BloomWalkStatsHandle, CommitGraphChain,
};
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{get_file_attrs, load_gitattributes, DiffAttr};
use grit_lib::diff::{
    count_changes, diff_trees, diff_trees_show_tree_entries, format_raw, unified_diff, zero_oid,
    DiffEntry, DiffStatus,
};
use grit_lib::diffstat::{terminal_columns, write_diffstat_block, DiffstatOptions, FileStatInput};
use grit_lib::git_date::parse::parse_date_basic;
use grit_lib::ident::{
    committer_timestamp_for_until_filter, committer_unix_seconds_for_ordering,
    parse_signature_tail, signature_timestamp_for_pretty, timestamp_for_at_ct, SignatureTail,
};
use grit_lib::line_log::{
    format_line_log_diff, line_log_filter_commits, parse_line_log_ranges, rewritten_first_parent,
};
use grit_lib::merge_base::is_ancestor;
use grit_lib::merge_diff::{
    blob_text_for_diff, blob_text_for_diff_with_oid, diff_textconv_active, is_binary_for_diff,
};
use grit_lib::objects::{parse_commit, parse_tag, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::reflog::read_reflog;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{
    collect_revision_specs_with_stdin, is_symmetric_diff, merge_bases, rev_list,
    split_symmetric_diff, OrderingMode, RevListOptions,
};
use grit_lib::rev_parse::{
    reflog_walk_refname, resolve_revision_as_commit, try_parse_double_dot_log_range,
};
use grit_lib::state::{resolve_head, HeadState};
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Arguments for `grit log`.
#[derive(Debug, ClapArgs)]
#[command(about = "Show commit logs")]
pub struct Args {
    /// Revisions and pathspecs (separated by --).
    pub revisions: Vec<String>,

    /// Limit the number of commits to show.
    #[arg(short = 'n', long = "max-count")]
    pub max_count: Option<usize>,

    /// Show only one line per commit.
    #[arg(long = "oneline")]
    pub oneline: bool,

    /// Pretty-print format.
    #[arg(long = "format", alias = "pretty")]
    pub format: Option<String>,

    /// Show in reverse order.
    #[arg(long = "reverse")]
    pub reverse: bool,

    /// Follow only the first parent of merge commits.
    #[arg(long = "first-parent")]
    pub first_parent: bool,

    /// Show root commits with diffs against an empty tree.
    #[arg(long = "root")]
    pub root: bool,

    /// Show a graph of the commit history.
    #[arg(long = "graph")]
    pub graph: bool,

    /// Decorate refs.
    #[arg(long = "decorate", overrides_with = "no_decorate")]
    pub decorate: Option<Option<String>>,

    /// Do not decorate refs.
    #[arg(long = "no-decorate", overrides_with = "decorate")]
    pub no_decorate: bool,

    /// Do not walk the commit graph — show given commits only.
    #[arg(long = "no-walk", default_missing_value = "sorted", num_args = 0..=1, require_equals = true)]
    pub no_walk: Option<String>,

    /// Show which ref led to each commit (with --all).
    #[arg(long = "source")]
    pub source: bool,

    /// Treat refs in alternate object stores as revision tips (`rev-list --alternate-refs`).
    #[arg(long = "alternate-refs")]
    pub alternate_refs: bool,

    /// Expanded from `git log --remotes[=pattern]` by the CLI preprocessor (hidden).
    #[arg(long = "grit-internal-remotes", hide = true)]
    pub internal_remotes_pattern: Option<String>,

    /// Only show commits on the ancestry path between endpoints.
    #[arg(long = "ancestry-path")]
    pub ancestry_path: bool,

    /// Only show commits that are decorated (have refs).
    #[arg(long = "simplify-by-decoration")]
    pub simplify_by_decoration: bool,

    /// Show full history (do not prune TREESAME merges).
    #[arg(long = "full-history")]
    pub full_history: bool,

    /// Further simplify full history by pruning redundant merges.
    #[arg(long = "simplify-merges")]
    pub simplify_merges: bool,

    /// Show all commits in simplified history mode.
    #[arg(long = "sparse")]
    pub sparse: bool,

    /// Show boundary commits.
    #[arg(long = "boundary")]
    pub boundary: bool,

    /// Show left/right markers for symmetric range (`A...B`).
    #[arg(long = "left-right")]
    pub left_right: bool,

    /// Show only commits reachable from the left side of `A...B`.
    #[arg(long = "left-only")]
    pub left_only: bool,

    /// Show only commits reachable from the right side of `A...B`.
    #[arg(long = "right-only")]
    pub right_only: bool,

    /// Skip this many commits.
    #[arg(long = "skip")]
    pub skip: Option<usize>,

    /// Filter by author (regex); multiple `--author` options are ORed.
    #[arg(long = "author", value_name = "PATTERN")]
    pub authors: Vec<String>,

    /// Filter by committer (regex); multiple options are ORed.
    #[arg(long = "committer", value_name = "PATTERN")]
    pub committers: Vec<String>,

    /// Skip merge commits.
    #[arg(long = "no-merges")]
    pub no_merges: bool,

    /// Show only merge commits.
    #[arg(long = "merges")]
    pub merges: bool,

    /// Date format.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Walk the reflog instead of the commit ancestry chain.
    #[arg(short = 'g', long = "walk-reflogs", alias = "reflog")]
    pub walk_reflogs: bool,

    /// Show unified diff (patch) after each commit.
    #[arg(short = 'p', long = "patch", alias = "unified")]
    pub patch: bool,

    /// Do not show diff after commits.
    #[arg(long = "no-patch")]
    pub no_patch: bool,

    /// Alias for --patch.
    #[arg(short = 'u', hide = true)]
    pub patch_u: bool,

    /// Show diffstat per commit (`--stat[=width[,name-width[,count]]]`).
    /// Repeatable like Git (`log -1 --stat --stat=60`).
    #[arg(
        long = "stat",
        num_args = 0..=1,
        default_missing_value = "",
        require_equals = true,
        action = clap::ArgAction::Append
    )]
    pub stat: Vec<String>,

    /// Limit the number of files shown in `--stat` output.
    #[arg(long = "stat-count")]
    pub stat_count: Option<usize>,

    /// Set the width of the `--stat` output.
    #[arg(long = "stat-width")]
    pub stat_width: Option<usize>,

    /// Set the width of the graph portion of `--stat` output.
    #[arg(long = "stat-graph-width")]
    pub stat_graph_width: Option<usize>,

    /// Set the width of the filename portion of `--stat` output.
    #[arg(long = "stat-name-width")]
    pub stat_name_width: Option<usize>,

    /// List changed file names per commit.
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Show status letter + filename per commit.
    #[arg(long = "name-status")]
    pub name_status: bool,

    /// Show raw diff-tree output per commit.
    #[arg(long = "raw")]
    pub raw: bool,

    /// Show log for all refs.
    #[arg(long = "all")]
    pub all: bool,

    /// Include all branch tips (refs/heads/) in the revision walk.
    #[arg(long = "branches")]
    pub branches: bool,

    /// Follow file renames (single file only).
    #[arg(long = "follow")]
    pub follow: bool,

    /// Filter by change type (A=added, M=modified, D=deleted, R=renamed, C=copied).
    #[arg(long = "diff-filter")]
    pub diff_filter: Option<String>,

    /// Only show commits that add or remove the given object.
    #[arg(long = "find-object")]
    pub find_object: Option<String>,

    /// Pickaxe extended-regex pattern (log `-G`; set via argv preprocessing).
    #[arg(long = "pickaxe-grep", value_name = "REGEX", hide = true)]
    pub pickaxe_grep: Option<String>,

    /// Pickaxe string (log `-S`; set via argv preprocessing).
    #[arg(long = "pickaxe-string", value_name = "STRING", hide = true)]
    pub pickaxe_string: Option<String>,

    /// Treat `-S` needle as an extended regex (`--pickaxe-regex`).
    #[arg(long = "pickaxe-regex", hide = true)]
    pub pickaxe_regex: bool,

    /// Force text semantics for pickaxe / binary handling (`-a` / `--text`).
    #[arg(short = 'a', long = "text")]
    pub text: bool,

    /// Run textconv when comparing blobs (default on for log pickaxe).
    #[arg(long = "textconv", hide = true)]
    pub textconv: bool,

    /// Disable textconv for pickaxe / diff.
    #[arg(long = "no-textconv", hide = true)]
    pub no_textconv: bool,

    /// Show full changeset when pickaxe matches (Git `--pickaxe-all`).
    #[arg(long = "pickaxe-all", hide = true)]
    pub pickaxe_all: bool,

    /// Rejected by Git (compatibility error).
    #[arg(long = "no-pickaxe-regex", hide = true)]
    pub no_pickaxe_regex: bool,

    /// Abbreviate commit hashes to N characters.
    #[arg(long = "abbrev", value_name = "N", default_missing_value = "7", num_args = 0..=1, require_equals = true)]
    pub abbrev: Option<String>,

    /// Use NUL as record terminator.
    #[arg(short = 'z')]
    pub null_terminator: bool,

    /// Suppress diff output for submodules.
    #[arg(long = "no-ext-diff")]
    pub no_ext_diff: bool,

    /// Show stat with patch.
    #[arg(long = "patch-with-stat")]
    pub patch_with_stat: bool,

    /// Disable rename detection.
    #[arg(long = "no-renames")]
    pub no_renames: bool,

    /// Detect renames.
    #[arg(short = 'M', long = "find-renames", default_missing_value = "50", num_args = 0..=1, require_equals = true)]
    pub find_renames: Option<String>,

    /// Detect copies.
    #[arg(short = 'C', long = "find-copies", default_missing_value = "50", num_args = 0..=1, require_equals = true)]
    pub find_copies: Option<String>,

    /// Control merge commit diff display.
    #[arg(long = "diff-merges", default_missing_value = "on")]
    pub diff_merges: Option<String>,

    /// Suppress diff output for merge commits.
    #[arg(long = "no-diff-merges")]
    pub no_diff_merges: bool,

    /// Show merge diffs in the default format (`log.diffMerges`, usually separate parents).
    ///
    /// Unlike `--diff-merges`, plain `-m` does not imply `--patch` by itself.
    #[arg(short = 'm')]
    pub merge_diff_m: bool,

    /// Combined diff for merge commits (shortcut for `--diff-merges=combined -p`).
    #[arg(short = 'c')]
    pub merge_diff_c: bool,

    /// Dense combined diff for merge commits (`--diff-merges=dense-combined -p`).
    #[arg(long = "cc")]
    pub cc: bool,

    /// First-parent merge diffs (shortcut for `--diff-merges=first-parent -p`).
    #[arg(long = "dd")]
    pub merge_diff_dd: bool,

    /// Show diff against a mechanical re-merge of the parents (two-parent merges).
    #[arg(long = "remerge-diff")]
    pub remerge_diff: bool,

    /// Color moved lines differently.
    #[arg(long = "color-moved", default_missing_value = "default", num_args = 0..=1, require_equals = true)]
    pub color_moved: Option<String>,

    /// Abbreviate commit hashes in output.
    #[arg(long = "abbrev-commit")]
    pub abbrev_commit: bool,

    /// Color output.
    #[arg(long = "color", default_missing_value = "always", num_args = 0..=1, require_equals = true)]
    pub color: Option<String>,

    /// Disable color.
    #[arg(long = "no-color")]
    pub no_color: bool,

    /// Filter decoration refs.
    #[arg(long = "decorate-refs", value_name = "PATTERN")]
    pub decorate_refs: Vec<String>,

    /// Exclude decoration refs.
    #[arg(long = "decorate-refs-exclude", value_name = "PATTERN")]
    pub decorate_refs_exclude: Vec<String>,

    /// Show line prefix.
    #[arg(long = "line-prefix", value_name = "PREFIX")]
    pub line_prefix: Option<String>,

    /// Disable graph output.
    #[arg(long = "no-graph")]
    pub no_graph: bool,

    /// Show a visual break between non-linear sections.
    #[arg(long = "show-linear-break", default_missing_value = "", num_args = 0..=1, require_equals = true)]
    pub show_linear_break: Option<String>,

    /// Show GPG signature.
    #[arg(long = "show-signature")]
    pub show_signature: bool,

    /// Disable abbreviation.
    #[arg(long = "no-abbrev")]
    pub no_abbrev: bool,

    /// Replace `+`/`-`/context prefixes in unified diff hunks (Git `range-diff` / `fast-import` tests).
    #[arg(long = "output-indicator-new", value_name = "C", hide = true)]
    pub output_indicator_new: Option<String>,

    #[arg(long = "output-indicator-old", value_name = "C", hide = true)]
    pub output_indicator_old: Option<String>,

    #[arg(long = "output-indicator-context", value_name = "C", hide = true)]
    pub output_indicator_context: Option<String>,

    /// Grep commit messages (regex unless `--fixed-strings`); multiple `--grep` are ORed unless
    /// `--all-match` requires every pattern to match.
    #[arg(long = "grep", value_name = "PATTERN")]
    pub grep_patterns: Vec<String>,

    /// Grep reflog messages (`log -g` only); multiple options are ORed unless `--all-match`.
    #[arg(long = "grep-reflog", value_name = "PATTERN")]
    pub grep_reflog_patterns: Vec<String>,

    /// Invert grep match.
    #[arg(long = "invert-grep")]
    pub invert_grep: bool,

    /// Case insensitive grep.
    #[arg(short = 'i', long = "regexp-ignore-case")]
    pub regexp_ignore_case: bool,

    /// All --grep patterns must match.
    #[arg(long = "all-match")]
    pub all_match: bool,

    /// Use basic regexp for --grep / --author / --committer (not pickaxe `-G`).
    #[arg(long = "basic-regexp")]
    pub basic_regexp: bool,

    /// Use extended regexp for --grep.
    #[arg(short = 'E', long = "extended-regexp")]
    pub extended_regexp: bool,

    /// Use fixed strings for --grep.
    #[arg(short = 'F', long = "fixed-strings")]
    pub fixed_strings: bool,

    /// Use Perl regexp for --grep.
    #[arg(short = 'P', long = "perl-regexp")]
    pub perl_regexp: bool,

    /// End of options marker (everything after is a revision/path).
    #[arg(long = "end-of-options")]
    pub end_of_options: bool,

    /// Date ordering.
    #[arg(long = "date-order")]
    pub date_order: bool,

    /// Order by author date instead of committer date.
    #[arg(long = "author-date-order")]
    pub author_date_order: bool,

    /// Topo ordering.
    #[arg(long = "topo-order")]
    pub topo_order: bool,

    /// Ignore missing refs.
    #[arg(long = "ignore-missing")]
    pub ignore_missing: bool,

    /// Clear all decorations.
    #[arg(long = "clear-decorations")]
    pub clear_decorations: bool,

    /// Show shortstat.
    #[arg(long = "shortstat")]
    pub shortstat: bool,

    /// Bisect mode (accepted for compatibility).
    #[arg(long = "bisect")]
    pub bisect: bool,

    /// Order files according to the given orderfile.
    #[arg(short = 'O', value_name = "orderfile")]
    pub order_file: Option<String>,

    /// Rotate diff output to start at the named path.
    #[arg(long = "rotate-to", value_name = "path")]
    pub rotate_to: Option<String>,

    /// Skip diff output until the named path.
    #[arg(long = "skip-to", value_name = "path")]
    pub skip_to: Option<String>,

    /// Show full object hashes in diff output.
    #[arg(long = "full-index")]
    pub full_index: bool,

    /// Omit `a/` and `b/` prefixes from diff paths (Git `--no-prefix`).
    #[arg(long = "no-prefix")]
    pub no_prefix: bool,

    /// Do not show commit notes (Git `log --no-notes`).
    #[arg(long = "no-notes")]
    pub no_notes: bool,

    /// Additional notes refs (`--notes` → default ref; `--notes=ref` → `refs/notes/<ref>`).
    #[arg(
        long = "notes",
        value_name = "REF",
        action = clap::ArgAction::Append,
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub notes_refs: Vec<String>,

    /// Show binary diffs in git-apply format.
    #[arg(long = "binary")]
    pub binary: bool,

    /// Filter: show commits newer than date (filter mode).
    #[arg(long = "since-as-filter", value_name = "DATE")]
    pub since_as_filter: Option<String>,

    /// Show commits newer than a specific date.
    #[arg(long = "since", alias = "after", value_name = "DATE")]
    pub since: Option<String>,

    /// Show commits older than a specific date.
    #[arg(long = "until", alias = "before", value_name = "DATE")]
    pub until: Option<String>,

    /// Annotate each commit with its children (accepted for compatibility).
    #[arg(long = "children")]
    pub children: bool,

    /// Pathspecs (after --).
    #[arg(last = true)]
    pub pathspecs: Vec<String>,

    /// Break complete rewrites into pairs.
    #[arg(short = 'B', long = "break-rewrites")]
    pub break_rewrites: Option<String>,

    /// Show tree objects in diff.
    #[arg(long = "show-trees")]
    pub show_trees: bool,

    /// Recurse into trees in diffs (`-t`, same as `git log -t`).
    #[arg(short = 't', hide = true)]
    pub recurse_trees: bool,

    /// Generate diff with N lines of context.
    #[arg(short = 'U', long = "unified", value_name = "N")]
    pub unified: Option<usize>,

    /// Trace line range history (`git log -L`).
    #[arg(short = 'L', value_name = "range:file", allow_hyphen_values = true)]
    pub line_range: Vec<String>,

    /// Print parent hashes on the first line of each commit (after rewrite).
    #[arg(long = "parents")]
    pub show_parents: bool,

    /// Write normal log output to a file; diff still goes to stdout.
    #[arg(long = "output", value_name = "file")]
    pub output_path: Option<PathBuf>,

    /// Suppress diff output (line-log: show commits only).
    #[arg(short = 's')]
    pub suppress_diff: bool,
}

/// How merge commits are diffed in `git log` (`--diff-merges`, `-m`, `-c`, `--cc`, `--dd`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergeDiffFormat {
    /// No merge diff (`--diff-merges=off` / default when no merge diff requested).
    Off,
    /// Diff only against the first parent (`--diff-merges=first-parent`, `--dd`, `--first-parent` default).
    FirstParent,
    /// One diff per parent (`separate`, plain `-m` with default `log.diffMerges`, `--diff-merges=on`).
    Separate,
    /// Combined diff (`-c`, `--diff-merges=combined`).
    Combined,
    /// Dense combined diff (`--cc`, `--diff-merges=dense-combined`).
    DenseCombined,
    /// Remerge diff (`--remerge-diff`); handled mainly before this enum is consulted.
    Remerge,
}

impl MergeDiffFormat {
    fn parse_diff_merges_cli_value(s: &str, default_on: MergeDiffFormat) -> Option<Self> {
        let s = s.trim();
        match s {
            "off" | "none" => Some(Self::Off),
            "1" | "first-parent" => Some(Self::FirstParent),
            "separate" => Some(Self::Separate),
            "c" | "combined" => Some(Self::Combined),
            "cc" | "dense-combined" => Some(Self::DenseCombined),
            "r" | "remerge" => Some(Self::Remerge),
            "m" | "on" => Some(default_on),
            _ => None,
        }
    }

    fn parse_log_diff_merges_config(s: &str) -> Result<Self> {
        let s = s.trim();
        match s {
            "off" | "none" => Ok(Self::Off),
            "1" | "first-parent" => Ok(Self::FirstParent),
            "separate" => Ok(Self::Separate),
            "c" | "combined" => Ok(Self::Combined),
            "cc" | "dense-combined" => Ok(Self::DenseCombined),
            "r" | "remerge" => Ok(Self::Remerge),
            // `m` / `on` in config are no-ops in C Git (leave default); initial default is separate.
            "m" | "on" => Ok(Self::Separate),
            _ => anyhow::bail!("fatal: bad config variable 'log.diffMerges' in file '.git/config'"),
        }
    }
}

fn log_diff_merges_default_format(git_dir: &Path) -> Result<MergeDiffFormat> {
    let cfg = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    if let Some(raw) = cfg
        .get("log.diffMerges")
        .or_else(|| cfg.get("log.diffmerges"))
    {
        MergeDiffFormat::parse_log_diff_merges_config(&raw)
    } else {
        Ok(MergeDiffFormat::Separate)
    }
}

fn validate_log_diff_merges_config(git_dir: &Path) -> Result<()> {
    let cfg = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    if let Some(raw) = cfg
        .get("log.diffMerges")
        .or_else(|| cfg.get("log.diffmerges"))
    {
        let _ = MergeDiffFormat::parse_log_diff_merges_config(&raw)?;
    }
    Ok(())
}

/// Apply `git log` merge-diff flags, validate config, and set implied `--patch` for `-c` / `--cc` / `--dd` / `--remerge-diff`.
fn normalize_log_merge_diff_args(args: &mut Args, git_dir: &Path) -> Result<()> {
    validate_log_diff_merges_config(git_dir)?;

    if (args.merge_diff_c || args.cc || args.merge_diff_dd || args.remerge_diff) && !args.no_patch {
        args.patch = true;
    }

    if args.merge_diff_c && args.cc {
        anyhow::bail!("options '-c' and '--cc' cannot be used together");
    }

    Ok(())
}

fn effective_merge_diff_format(
    args: &Args,
    is_merge: bool,
    git_dir: &Path,
) -> Result<MergeDiffFormat> {
    if !is_merge {
        return Ok(MergeDiffFormat::FirstParent);
    }
    if args.remerge_diff {
        return Ok(MergeDiffFormat::Remerge);
    }
    if args.merge_diff_c {
        return Ok(MergeDiffFormat::Combined);
    }
    if args.cc {
        return Ok(MergeDiffFormat::DenseCombined);
    }
    if args.merge_diff_dd {
        return Ok(MergeDiffFormat::FirstParent);
    }
    if args.no_diff_merges {
        return Ok(MergeDiffFormat::Off);
    }
    if let Some(ref s) = args.diff_merges {
        let default_on = log_diff_merges_default_format(git_dir)?;
        if let Some(fmt) = MergeDiffFormat::parse_diff_merges_cli_value(s, default_on) {
            return Ok(fmt);
        }
        anyhow::bail!("invalid value for '--diff-merges': '{s}'");
    }
    if args.merge_diff_m {
        return log_diff_merges_default_format(git_dir);
    }
    if args.first_parent {
        return Ok(MergeDiffFormat::FirstParent);
    }
    Ok(MergeDiffFormat::Off)
}

/// Whether a merge commit should emit any diff (stat/raw/patch) for the current log options.
fn merge_commit_wants_diff(args: &Args, git_dir: &Path) -> Result<bool> {
    Ok(effective_merge_diff_format(args, true, git_dir)? != MergeDiffFormat::Off)
}

/// Whether `git log` should run diff machinery for a commit (false for merge + `off` unless only `-m` without patch — then still false).
fn log_commit_needs_diff_output(args: &Args, info: &CommitInfo, git_dir: &Path) -> Result<bool> {
    let wants_diff = args.patch
        || args.patch_u
        || !args.stat.is_empty()
        || args.name_only
        || args.name_status
        || args.raw
        || args.cc
        || args.merge_diff_c
        || args.remerge_diff
        || args.patch_with_stat;
    if !wants_diff {
        return Ok(false);
    }
    let is_merge = info.parents.len() > 1;
    if is_merge && !merge_commit_wants_diff(args, git_dir)? {
        return Ok(false);
    }
    Ok(true)
}

/// Whether unified patch hunks should be printed (honors `-m` not implying `-p` alone).
fn log_wants_patch_hunks(args: &Args, info: &CommitInfo, git_dir: &Path) -> Result<bool> {
    if args.no_patch || args.suppress_diff {
        return Ok(false);
    }
    let is_merge = info.parents.len() > 1;
    let patch = args.patch || args.patch_u;
    if !patch {
        return Ok(false);
    }
    if is_merge && effective_merge_diff_format(args, true, git_dir)? == MergeDiffFormat::Off {
        return Ok(false);
    }
    Ok(true)
}

/// Whether combined-style merge diff is active (`-c` / `--cc`).
fn merge_diff_is_combined_style(args: &Args, is_merge: bool, git_dir: &Path) -> Result<bool> {
    if !is_merge {
        return Ok(false);
    }
    let f = effective_merge_diff_format(args, true, git_dir)?;
    Ok(matches!(
        f,
        MergeDiffFormat::Combined | MergeDiffFormat::DenseCombined
    ))
}

/// Whether log should use the dense combined diff implementation (`--cc`).
fn merge_diff_is_dense_combined(args: &Args, is_merge: bool, git_dir: &Path) -> Result<bool> {
    if !is_merge {
        return Ok(false);
    }
    Ok(effective_merge_diff_format(args, true, git_dir)? == MergeDiffFormat::DenseCombined)
}

/// Whether log should emit one diff per parent (`separate` / `-m` default).
fn merge_diff_is_separate(args: &Args, is_merge: bool, git_dir: &Path) -> Result<bool> {
    if !is_merge {
        return Ok(false);
    }
    Ok(effective_merge_diff_format(args, true, git_dir)? == MergeDiffFormat::Separate)
}

/// Whether log should emit a remerge diff for merges.
fn merge_diff_is_remerge(args: &Args, is_merge: bool, git_dir: &Path) -> Result<bool> {
    if !is_merge {
        return Ok(false);
    }
    Ok(effective_merge_diff_format(args, true, git_dir)? == MergeDiffFormat::Remerge)
}

/// Whether log/show diff output should use ANSI colors on stdout (Git `color.diff` / `color.ui`).
fn log_resolve_stdout_color(args: &Args, git_dir: &Path) -> bool {
    if args.no_color {
        return false;
    }
    if let Some(ref c) = args.color {
        return c == "always" || c == "true" || c.is_empty();
    }
    let mut c = false;
    if let Ok(config) = ConfigSet::load(Some(git_dir), true) {
        if let Some(val) = config.get("color.diff") {
            match val.as_str() {
                "always" | "true" => c = true,
                "auto" => {
                    c = std::io::IsTerminal::is_terminal(&std::io::stdout())
                        || std::env::var_os("GIT_PAGER_IN_USE").is_some()
                }
                _ => {}
            }
        }
        if !c {
            if let Some(val) = config.get("color.ui") {
                match val.as_str() {
                    "always" | "true" => c = true,
                    "auto" => {
                        c = std::io::IsTerminal::is_terminal(&std::io::stdout())
                            || std::env::var_os("GIT_PAGER_IN_USE").is_some()
                    }
                    _ => {}
                }
            }
        }
    }
    c
}

fn run_line_log(repo: &Repository, args: Args, _patch_context: usize) -> Result<()> {
    if !args.pathspecs.is_empty() {
        anyhow::bail!("-L<range>:<file> cannot be used with pathspec");
    }
    if args.follow {
        anyhow::bail!("options '-L' and '--follow' cannot be used together");
    }
    if args.raw {
        anyhow::bail!("--raw is incompatible with -L");
    }
    if args.reverse && args.graph {
        anyhow::bail!("options '--reverse' and '--graph' cannot be used together");
    }

    let use_color = if args.no_color {
        false
    } else if let Some(ref c) = args.color {
        c == "always" || c == "true" || c.is_empty()
    } else {
        let mut c = false;
        if let Ok(config) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true) {
            if let Some(val) = config.get("color.diff") {
                match val.as_str() {
                    "always" | "true" => c = true,
                    "auto" => {
                        c = std::io::IsTerminal::is_terminal(&std::io::stdout())
                            || std::env::var_os("GIT_PAGER_IN_USE").is_some()
                    }
                    _ => {}
                }
            }
        }
        c
    };

    let (start_oids, exclude_oids) = if args.all {
        (collect_all_ref_oids(&repo.git_dir)?, Vec::new())
    } else if args.revisions.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        match head.oid() {
            Some(oid) => (vec![*oid], Vec::new()),
            None => anyhow::bail!("your current branch does not have any commits yet"),
        }
    } else {
        let mut oids = Vec::new();
        let mut excludes = Vec::new();
        for rev in &args.revisions {
            if let Some(stripped) = rev.strip_prefix('^') {
                excludes.push(resolve_revision_as_commit(repo, stripped)?);
            } else if let Some((excl, tip)) = try_parse_double_dot_log_range(repo, rev)? {
                excludes.push(excl);
                oids.push(tip);
            } else {
                oids.push(resolve_revision_as_commit(repo, rev)?);
            }
        }
        if oids.is_empty() {
            let head = resolve_head(&repo.git_dir)?;
            if let Some(oid) = head.oid() {
                oids.push(*oid);
            }
        }
        (oids, excludes)
    };

    if start_oids.len() != 1 {
        anyhow::bail!("More than one commit to dig from");
    }
    let tip = start_oids[0];

    let excluded_set: HashSet<ObjectId> = if exclude_oids.is_empty() {
        HashSet::new()
    } else {
        collect_reachable(&repo.odb, &exclude_oids)?
    };

    let rename_threshold = args
        .find_renames
        .as_deref()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(50);

    let walk = walk_commits(
        &repo.odb,
        &repo.git_dir,
        &[tip],
        None,
        args.skip,
        args.first_parent,
        &[],
        &[],
        &[],
        false,
        false,
        args.no_merges,
        args.merges,
        &[][..],
        &excluded_set,
        None,
        None,
        true,
        -1,
        None,
        &[][..],
        None,
        false,
    )?;
    let order: Vec<ObjectId> = walk.iter().map(|(o, _)| *o).collect();

    let initial = parse_line_log_ranges(
        &repo.odb,
        &repo.git_dir,
        repo.work_tree.as_deref(),
        &tip,
        &args.line_range,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    let (filtered, _state, displays) = line_log_filter_commits(
        &repo.odb,
        order,
        tip,
        initial,
        rename_threshold,
        args.first_parent,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut filtered = filtered;
    if let Some(threshold) = args.until.as_ref().and_then(|s| parse_date_to_epoch(s)) {
        filtered.retain(|oid| {
            load_commit_info(repo, *oid)
                .map(|info| extract_epoch_from_ident(&info.committer) <= threshold)
                .unwrap_or(false)
        });
    }

    let filtered_set: HashSet<ObjectId> = filtered.iter().copied().collect();

    let mut out_main: Box<dyn Write> = if let Some(ref p) = args.output_path {
        Box::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(p)
                .with_context(|| format!("open --output {}", p.display()))?,
        )
    } else {
        Box::new(io::stdout().lock())
    };

    let line_prefix = args.line_prefix.as_deref().unwrap_or("");
    let mut notes_cache = NotesMapCache::new(repo);

    let format_requires_decorations = args
        .format
        .as_deref()
        .map(|fmt| {
            let template = fmt
                .strip_prefix("format:")
                .or_else(|| fmt.strip_prefix("tformat:"))
                .unwrap_or(fmt);
            template.contains("%d") || template.contains("%D")
        })
        .unwrap_or(false);
    let (show_decorations, decorate_full) =
        resolve_decoration_display(&args, format_requires_decorations);
    let decorations = if !show_decorations {
        None
    } else {
        Some(collect_decorations(repo, decorate_full)?)
    };

    let show_patch = !args.suppress_diff && !args.no_patch;

    let (graph_line_log_pipe_pfx, graph_line_log_space_pfx) = if args.graph && show_patch {
        let use_color_g = log_resolve_stdout_color(&args, &repo.git_dir);
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let red = if use_color_g {
            cfg.get("color.diff.meta")
                .and_then(|s| grit_lib::config::parse_color(&s).ok())
                .unwrap_or_else(|| "\x1b[31m".to_string())
        } else {
            String::new()
        };
        let reset = if use_color_g { "\x1b[m" } else { "" };
        (
            Some(format!("{line_prefix}{red}|{reset} ")),
            Some(format!("{line_prefix}  ")),
        )
    } else {
        (None, None)
    };

    if args.graph {
        let mut nodes = Vec::new();
        let mut seen = HashSet::new();
        for oid in &filtered {
            if !seen.insert(*oid) {
                continue;
            }
            let parents: Vec<ObjectId> = load_raw_parents(repo, *oid)?
                .into_iter()
                .filter(|p| filtered_set.contains(p))
                .collect();
            nodes.push(GraphCommitNode {
                oid: *oid,
                parents,
                is_boundary: false,
            });
        }

        let abbrev_len = parse_abbrev(&args.abbrev);
        let mut graph = AsciiGraph::new();

        let node_count = nodes.len();
        for (node_idx, node) in nodes.into_iter().enumerate() {
            let info = load_commit_info(repo, node.oid)?;
            graph.update(node.clone());

            loop {
                let (line, shown_commit_line) = graph.next_line();
                if shown_commit_line {
                    let parent_line: Vec<ObjectId> = if args.show_parents {
                        rewritten_first_parent(&repo.odb, &node.oid, &filtered_set)
                            .map_err(|e| anyhow::anyhow!("{}", e))?
                            .into_iter()
                            .collect()
                    } else {
                        node.parents.clone()
                    };
                    let rendered = render_graph_commit_text(
                        &node,
                        &info,
                        &args,
                        decorations.as_ref(),
                        abbrev_len,
                        &parent_line,
                    );
                    writeln!(out_main, "{line_prefix}{line}{rendered}")?;
                    break;
                }
                writeln!(out_main, "{line_prefix}{line}")?;
            }

            while !graph.is_commit_finished() {
                let (line, _) = graph.next_line();
                writeln!(out_main, "{line_prefix}{line}")?;
            }

            if show_patch {
                let nparents = load_raw_parents(repo, node.oid)?.len();
                if nparents <= 1 {
                    if let Some(ds) = displays.get(&node.oid) {
                        let diff_pfx = match graph_line_log_pipe_pfx.as_deref() {
                            Some(pipe) if node_idx == 0 => pipe,
                            Some(_) => graph_line_log_space_pfx.as_deref().unwrap_or(line_prefix),
                            None => line_prefix,
                        };
                        for d in ds {
                            write!(
                                out_main,
                                "{}",
                                format_line_log_diff(
                                    diff_pfx,
                                    &d.old_path,
                                    &d.new_path,
                                    &d.old_bytes,
                                    &d.new_bytes,
                                    &d.commit_ranges,
                                    &d.touched,
                                )
                            )?;
                        }
                    }
                } else if node_idx + 1 < node_count {
                    writeln!(out_main)?;
                    writeln!(out_main)?;
                }
            }
        }
        return Ok(());
    }

    let is_format_separator = args
        .format
        .as_deref()
        .map(|f| f.starts_with("format:"))
        .unwrap_or(false);

    let n_filtered = filtered.len();
    let mut prev_had_notes = false;
    for (i, oid) in filtered.iter().enumerate() {
        if is_format_separator && i > 0 {
            if args.null_terminator {
                write!(out_main, "\0")?;
            } else {
                writeln!(out_main)?;
            }
        }
        let this_has_notes = commit_has_notes_to_show(oid, &mut notes_cache, &args);
        if !is_format_separator && i > 0 && prev_had_notes {
            writeln!(out_main)?;
        }
        let info = load_commit_info(repo, *oid)?;
        let parent_override: Option<Vec<ObjectId>> = if args.show_parents {
            rewritten_first_parent(&repo.odb, oid, &filtered_set)
                .map_err(|e| anyhow::anyhow!("{}", e))?
                .map(|p| vec![p])
        } else {
            None
        };
        format_commit(
            &mut out_main,
            oid,
            &info,
            &args,
            decorations.as_ref(),
            use_color,
            &mut notes_cache,
            &repo.odb,
            parent_override.as_deref(),
            true,
            None,
            None,
            None,
        )?;
        prev_had_notes = this_has_notes;
        let nparents = load_raw_parents(repo, *oid)?.len();
        if show_patch {
            if nparents <= 1 {
                if let Some(ds) = displays.get(oid) {
                    for d in ds {
                        write!(
                            out_main,
                            "{}",
                            format_line_log_diff(
                                line_prefix,
                                &d.old_path,
                                &d.new_path,
                                &d.old_bytes,
                                &d.new_bytes,
                                &d.commit_ranges,
                                &d.touched,
                            )
                        )?;
                    }
                    if i + 1 < n_filtered {
                        writeln!(out_main)?;
                    }
                }
            } else if i + 1 < n_filtered {
                // Git prints an extra blank line after merge commits when emitting line-log patches.
                writeln!(out_main)?;
                writeln!(out_main)?;
            }
        }
    }

    Ok(())
}

/// Extract epoch timestamp from a Git ident string.
fn sort_revision_specs_by_committer_desc(repo: &Repository, specs: &mut Vec<String>) -> Result<()> {
    use std::cmp::Reverse;
    let mut metas: Vec<(Reverse<i64>, String)> = Vec::with_capacity(specs.len());
    for s in specs.drain(..) {
        let oid = resolve_revision_as_commit(repo, &s)?;
        let obj = repo.odb.read(&oid)?;
        let c = parse_commit(&obj.data)?;
        let e = extract_epoch_from_ident(&c.committer);
        metas.push((Reverse(e), s));
    }
    metas.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    specs.extend(metas.into_iter().map(|(_, s)| s));
    Ok(())
}

fn extract_epoch_from_ident(ident: &str) -> i64 {
    committer_timestamp_for_until_filter(ident)
}

/// When `git log --graph <tip> --branches` is used, Git prefers `<tip>` as the leftmost
/// branch tip when it is incomparable with the current first commit (t3451-history-reword).
/// `git log --graph --branches` with no explicit revisions: walk `HEAD`'s first-parent chain and,
/// at each step, list parallel branch tips (descendants of the next FP parent that are not
/// ancestors of the current FP commit). Order the current commit vs parallel tips by comparing
/// the lexicographically smallest local branch name at each tip (`t3452-history-split` tests 8,
/// 10, 11).
fn reorder_graph_all_branches_no_explicit_rev(
    repo: &Repository,
    commits: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let head = grit_lib::refs::resolve_ref(&repo.git_dir, "HEAD")?;
    let set: std::collections::HashSet<ObjectId> = commits.iter().copied().collect();
    if !set.contains(&head) {
        return Ok(commits.to_vec());
    }

    let mut tips_at_oid: std::collections::HashMap<ObjectId, Vec<String>> =
        std::collections::HashMap::new();
    for (refname, oid) in grit_lib::refs::list_refs(&repo.git_dir, "refs/heads/")? {
        let short = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(&refname)
            .to_owned();
        tips_at_oid.entry(oid).or_default().push(short);
    }
    for v in tips_at_oid.values_mut() {
        v.sort();
    }
    let min_branch = |oid: ObjectId| -> String {
        tips_at_oid
            .get(&oid)
            .and_then(|v| v.first().cloned())
            .unwrap_or_else(|| "\u{10ffff}".to_owned())
    };
    let sort_parallel = |repo: &Repository, v: &mut Vec<ObjectId>| {
        v.sort_by(|a, b| {
            let ta = read_commit_timestamp(&repo.odb, a);
            let tb = read_commit_timestamp(&repo.odb, b);
            tb.cmp(&ta).then_with(|| b.cmp(a))
        });
    };

    let mut fp_chain: Vec<ObjectId> = Vec::new();
    let mut cur = head;
    loop {
        if !set.contains(&cur) {
            break;
        }
        fp_chain.push(cur);
        let obj = repo.odb.read(&cur)?;
        let c = parse_commit(&obj.data)?;
        let Some(&p) = c.parents.first() else {
            break;
        };
        cur = p;
    }

    let on_fp: std::collections::HashSet<ObjectId> = fp_chain.iter().copied().collect();
    let mut out: Vec<ObjectId> = Vec::with_capacity(commits.len());
    let mut used: std::collections::HashSet<ObjectId> = std::collections::HashSet::new();

    for window in fp_chain.windows(2) {
        let cur_win = window[0];
        let next_win = window[1];

        let mut side: Vec<ObjectId> = commits
            .iter()
            .copied()
            .filter(|&oid| {
                if oid == next_win || oid == cur_win || on_fp.contains(&oid) || used.contains(&oid)
                {
                    return false;
                }
                is_ancestor(repo, next_win, oid).unwrap_or(false)
                    && !is_ancestor(repo, oid, cur_win).unwrap_or(false)
            })
            .collect();

        let cur_key = min_branch(cur_win);
        let cur_has_branch = tips_at_oid.contains_key(&cur_win);
        let side_key = side
            .iter()
            .map(|&o| min_branch(o))
            .min()
            .unwrap_or_else(|| "\u{10ffff}".to_owned());
        sort_parallel(repo, &mut side);

        // Intermediate FP commits often have no refs pointing at them; keep the main line first
        // before parallel tips (t3452 test 8: ours-b → ours-a → split-me).
        let side_first = !side.is_empty() && cur_has_branch && side_key < cur_key;

        if side_first {
            for oid in &side {
                if used.insert(*oid) {
                    out.push(*oid);
                }
            }
            if used.insert(cur_win) {
                out.push(cur_win);
            }
        } else {
            if used.insert(cur_win) {
                out.push(cur_win);
            }
            for oid in side {
                if used.insert(oid) {
                    out.push(oid);
                }
            }
        }
    }

    if let Some(&last) = fp_chain.last() {
        if used.insert(last) {
            out.push(last);
        }
    }

    for &oid in commits {
        if !used.contains(&oid) {
            out.push(oid);
        }
    }
    Ok(out)
}

fn prefer_explicit_tip_first_in_graph_walk(
    repo: &Repository,
    args: &Args,
    commits: &mut Vec<ObjectId>,
) -> Result<()> {
    let Some(tip_spec) = args
        .revisions
        .iter()
        .find(|r| *r != "--" && !r.starts_with('-'))
    else {
        return Ok(());
    };
    let tip_oid = resolve_revision_as_commit(repo, tip_spec)?;
    let Some(&first) = commits.first() else {
        return Ok(());
    };
    if first == tip_oid {
        return Ok(());
    }
    let Some(pos) = commits.iter().position(|&c| c == tip_oid) else {
        return Ok(());
    };
    if pos == 0 {
        return Ok(());
    }
    let incomparable = !is_ancestor(repo, first, tip_oid)? && !is_ancestor(repo, tip_oid, first)?;
    if !incomparable {
        return Ok(());
    }
    commits.remove(pos);
    commits.insert(0, tip_oid);
    Ok(())
}

/// Parse a date string into a Unix epoch timestamp.
fn parse_date_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok((ts, _off_min)) = parse_date_basic(s) {
        return i64::try_from(ts).ok();
    }
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        let parts: Vec<&str> = s[..10].split('-').collect();
        if parts.len() == 3 {
            if let (Ok(y), Ok(m), Ok(d)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            ) {
                if let Ok(month) = time::Month::try_from(m) {
                    if let Ok(date) = time::Date::from_calendar_date(y, month, d) {
                        let dt = date.with_hms(0, 0, 0).unwrap().assume_utc();
                        return Some(dt.unix_timestamp());
                    }
                }
            }
        }
    }
    s.parse::<i64>().ok()
}

/// Whether to load ref decorations and whether to use full ref names (`refs/heads/...`).
///
/// Mirrors Git's handling of `--decorate`, `--no-decorate`, and raw argv scanning for
/// those flags. `--oneline` does not imply `--decorate`; use `--decorate` or a format
/// with `%d` / `%D` when decorations are required.
fn resolve_decoration_display(args: &Args, format_requires_decorations: bool) -> (bool, bool) {
    let mut show = format_requires_decorations;
    // Git enables decorations for `%d` / `%D` with short ref names; `--decorate=full` opts in.
    let mut full = false;
    for arg in std::env::args() {
        if arg == "--no-decorate" {
            show = false;
            full = false;
        } else if arg.starts_with("--decorate") {
            show = true;
            full = arg == "--decorate=full";
        }
    }
    if args.decorate.is_some() {
        show = true;
        if let Some(Some(mode)) = &args.decorate {
            if mode == "full" {
                full = true;
            }
        }
    }
    if args.no_decorate {
        show = false;
        full = false;
    }
    let oneline_like = args.oneline || args.format.as_deref() == Some("oneline");
    if oneline_like && !args.no_decorate && !show {
        // Upstream Git only decorates `--oneline` for TTY/pager output, but many harness tests
        // (and users comparing to `git log` in scripts) expect branch tips on every line; match
        // that behavior unless `--no-decorate` is set.
        show = true;
        full = false;
    }
    (show, full)
}

/// Emit `git log --graph`-style output: the graph commit row (`*` / merge) shares its line with
/// the first pretty line (`commit …` or `format:` output); each following body line is prefixed
/// with another graph row (upstream `graph_show_strbuf` + `graph_show_commit_msg`).
fn write_graph_interleaved_commit_msg(
    out: &mut impl Write,
    line_prefix: &str,
    graph_commit_line: &str,
    graph: &mut AsciiGraph,
    body: &str,
) -> Result<()> {
    let newline_terminated = !body.is_empty() && body.ends_with('\n');
    let trimmed = body.trim_end_matches('\n');
    if !trimmed.contains('\n')
        && trimmed.len() == 40
        && trimmed.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    {
        writeln!(out, "{line_prefix}{graph_commit_line}")?;
        writeln!(out, "{line_prefix}{trimmed}")?;
        if !graph.is_commit_finished() {
            if !newline_terminated {
                writeln!(out)?;
            }
            graph_show_remainder_lines(out, line_prefix, graph)?;
            if newline_terminated {
                writeln!(out)?;
            }
        } else if !newline_terminated {
            writeln!(out)?;
        }
        return Ok(());
    }

    let mut lines = body.split_inclusive('\n').peekable();
    let Some(first_chunk) = lines.next() else {
        writeln!(out, "{line_prefix}{graph_commit_line}")?;
        graph_show_remainder_lines(out, line_prefix, graph)?;
        return Ok(());
    };

    let first_line = first_chunk.strip_suffix('\n').unwrap_or(first_chunk);
    write!(out, "{line_prefix}{graph_commit_line}{first_line}")?;
    if first_chunk.ends_with('\n') {
        writeln!(out)?;
    }

    for chunk in lines {
        let text = chunk.strip_suffix('\n').unwrap_or(chunk);
        if !graph.is_commit_finished() {
            let (gline, _) = graph.next_line();
            write!(out, "{line_prefix}{gline}")?;
        }
        write!(out, "{text}")?;
        if chunk.ends_with('\n') {
            writeln!(out)?;
        }
    }

    if !graph.is_commit_finished() {
        if !newline_terminated {
            writeln!(out)?;
        }
        graph_show_remainder_lines(out, line_prefix, graph)?;
        if newline_terminated {
            writeln!(out)?;
        }
    } else if !newline_terminated && !body.is_empty() {
        writeln!(out)?;
    }
    Ok(())
}

fn graph_show_remainder_lines(
    out: &mut impl Write,
    line_prefix: &str,
    graph: &mut AsciiGraph,
) -> Result<()> {
    while !graph.is_commit_finished() {
        let (gline, _) = graph.next_line();
        writeln!(out, "{line_prefix}{gline}")?;
    }
    Ok(())
}

fn run_graph_log(repo: &Repository, args: &Args, patch_context: usize) -> Result<()> {
    let mut implied_pathspecs: Vec<String> = Vec::new();
    let mut revision_specs = Vec::new();
    for rev in &args.revisions {
        if rev == "--" {
            break;
        }
        if rev.starts_with('-') && !rev.starts_with('^') {
            continue;
        }
        if let Some(stripped) = rev.strip_prefix('^') {
            match resolve_revision_as_commit(repo, stripped) {
                Ok(_) => revision_specs.push(rev.clone()),
                Err(_err) if is_likely_pathspec_during_rev_parse(stripped) => {
                    implied_pathspecs.push(stripped.to_owned())
                }
                Err(_err) => match resolve_revision_as_commit_after_precompose(repo, stripped) {
                    Ok(_) => revision_specs.push(rev.clone()),
                    Err(_err)
                        if grit_lib::precompose_config::effective_core_precomposeunicode(Some(
                            &repo.git_dir,
                        )) && grit_lib::unicode_normalization::has_non_ascii_utf8(stripped) =>
                    {
                        implied_pathspecs.push(stripped.to_owned());
                    }
                    Err(err) => return Err(err.into()),
                },
            }
        } else {
            match resolve_revision_as_commit(repo, rev) {
                Ok(_) => revision_specs.push(rev.clone()),
                Err(_err) if is_likely_pathspec_during_rev_parse(rev) => {
                    implied_pathspecs.push(rev.clone())
                }
                Err(_err) => match resolve_revision_as_commit_after_precompose(repo, rev) {
                    Ok(_) => revision_specs.push(rev.clone()),
                    Err(_err)
                        if grit_lib::precompose_config::effective_core_precomposeunicode(Some(
                            &repo.git_dir,
                        )) && grit_lib::unicode_normalization::has_non_ascii_utf8(rev) =>
                    {
                        implied_pathspecs.push(rev.clone());
                    }
                    Err(err) => return Err(err.into()),
                },
            }
        }
    }

    let user_revision_specs_len = revision_specs.len();

    if args.branches {
        let mut seen: std::collections::HashSet<String> = revision_specs.iter().cloned().collect();
        for (_, oid) in grit_lib::refs::list_refs(&repo.git_dir, "refs/heads/")? {
            let s = oid.to_hex();
            if seen.insert(s.clone()) {
                revision_specs.push(s);
            }
        }
    }

    if !implied_pathspecs.is_empty() {
        validate_pathspec_scope(repo, &implied_pathspecs)?;
    }

    let mut combined_pathspecs = args.pathspecs.clone();
    combined_pathspecs.extend(implied_pathspecs);
    combined_pathspecs = resolve_effective_pathspecs(repo, &combined_pathspecs)?;

    let mut options = RevListOptions {
        all_refs: args.all,
        first_parent: args.first_parent,
        simplify_by_decoration: false,
        skip: args.skip.unwrap_or(0),
        max_count: args.max_count,
        ordering: if args.date_order {
            OrderingMode::Date
        } else {
            OrderingMode::Topo
        },
        reverse: false,
        boundary: args.boundary,
        full_history: args.full_history,
        sparse: args.sparse,
        paths: if args.follow {
            Vec::new()
        } else {
            combined_pathspecs.clone()
        },
        ..RevListOptions::default()
    };
    if args.no_merges {
        options.max_parents = Some(1);
    }
    if args.merges {
        options.min_parents = Some(2);
    }

    let (mut positive_specs, negative_specs, stdin_all_refs) =
        collect_revision_specs_with_stdin(&revision_specs, false)
            .map_err(|e| anyhow::anyhow!("failed to parse revision arguments: {e}"))?;
    if stdin_all_refs {
        options.all_refs = true;
    }

    if positive_specs.is_empty() && !options.all_refs {
        positive_specs.push("HEAD".to_owned());
    }

    if args.branches {
        sort_revision_specs_by_committer_desc(repo, &mut positive_specs)?;
    }

    let mut result = rev_list(repo, &positive_specs, &negative_specs, &options)
        .map_err(|e| anyhow::anyhow!("rev-list failed: {e}"))?;

    if args.branches {
        prefer_explicit_tip_first_in_graph_walk(repo, args, &mut result.commits)?;
    }

    if args.branches && user_revision_specs_len == 0 {
        result.commits = reorder_graph_all_branches_no_explicit_rev(repo, &result.commits)?;
    }

    if args.simplify_by_decoration {
        result.commits = simplify_by_decoration_for_graph(repo, &result.commits)?;
    }

    if args.simplify_merges && args.full_history {
        let simplified = simplify_merges_for_graph(repo, &result.commits)?;
        result.commits = simplified;
    }

    if !combined_pathspecs.is_empty() && !args.full_history {
        if args.sparse {
            let mut dense_options = options.clone();
            dense_options.sparse = false;
            let dense_result = rev_list(repo, &positive_specs, &negative_specs, &dense_options)
                .map_err(|e| anyhow::anyhow!("rev-list failed: {e}"))?;
            let dense_ordered =
                reorder_path_limited_graph_commits(repo, &dense_result.commits, args.first_parent)?;
            result.commits = expand_sparse_path_limited_graph_history(repo, &dense_ordered)?;
        } else {
            // Same commit set as `rev-list`; reorder only for graph column layout (main line before
            // side branches), matching `git log --graph -- <paths>` (`t6016`).
            result.commits =
                reorder_path_limited_graph_commits(repo, &result.commits, args.first_parent)?;
        }
    }

    let included: HashSet<ObjectId> = result.commits.iter().copied().collect();
    let ordered_boundaries = if args.boundary {
        order_boundary_commits_for_graph(
            repo,
            &result.boundary_commits,
            result.commits.first().copied(),
        )?
    } else {
        Vec::new()
    };
    let mut graph_parent_targets = included.clone();
    graph_parent_targets.extend(ordered_boundaries.iter().copied());
    let simplify_graph_parents =
        args.simplify_by_decoration && combined_pathspecs.is_empty() && !args.full_history;
    // Path-limited history: when walking through commits omitted from the simplified list,
    // follow only the first parent so graph edges match Git's parent rewriting for `--graph`.
    // `--full-history` alone keeps full parent connectivity (t6016 case 6); with
    // `--full-history --simplify-merges` Git again collapses through omitted merges (t6016 case 7).
    let fp_through_omitted_for_graph =
        !combined_pathspecs.is_empty() && (!args.full_history || args.simplify_merges);
    // `--sparse` path-limited graph: show the first-parent spine as a straight column (t6016).
    let graph_first_parent_direct =
        args.first_parent || (args.sparse && !combined_pathspecs.is_empty() && !args.full_history);
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();

    for oid in &result.commits {
        if !seen.insert(*oid) {
            continue;
        }
        let parents = visible_parents_for_graph(
            repo,
            *oid,
            &graph_parent_targets,
            graph_first_parent_direct,
            fp_through_omitted_for_graph,
            simplify_graph_parents,
        )?;
        nodes.push(GraphCommitNode {
            oid: *oid,
            parents,
            is_boundary: false,
        });
    }

    if args.boundary {
        for oid in &ordered_boundaries {
            if !seen.insert(*oid) {
                continue;
            }
            let mut parents = load_raw_parents(repo, *oid)?;
            if args.first_parent && parents.len() > 1 {
                parents.truncate(1);
            }
            nodes.push(GraphCommitNode {
                oid: *oid,
                parents,
                is_boundary: true,
            });
        }
    }

    let interesting: HashSet<ObjectId> = nodes.iter().map(|n| n.oid).collect();
    for node in &mut nodes {
        node.parents.retain(|p| interesting.contains(p));
    }

    let format_requires_decorations_graph = args
        .format
        .as_deref()
        .map(|fmt| {
            let template = fmt
                .strip_prefix("format:")
                .or_else(|| fmt.strip_prefix("tformat:"))
                .unwrap_or(fmt);
            template.contains("%d") || template.contains("%D")
        })
        .unwrap_or(false);
    let (show_decorations_graph, decorate_full_graph) =
        resolve_decoration_display(args, format_requires_decorations_graph);
    let decorations = if args.simplify_by_decoration || show_decorations_graph {
        Some(collect_decorations(repo, decorate_full_graph)?)
    } else {
        None
    };

    let mut notes_cache = NotesMapCache::new(repo);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut graph = AsciiGraph::new();
    let line_prefix = args.line_prefix.as_deref().unwrap_or("");
    let abbrev_len = parse_abbrev(&args.abbrev);
    let use_color = log_resolve_stdout_color(args, &repo.git_dir);
    let show_commit_body = !args.suppress_diff
        && !args.no_patch
        && (args.patch
            || args.patch_u
            || args.patch_with_stat
            || !args.stat.is_empty()
            || args.name_only
            || args.name_status
            || args.raw
            || args.cc
            || args.merge_diff_c
            || args.remerge_diff);

    let graph_stat_prefix: Option<String> = if show_commit_body && !args.stat.is_empty() {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let red = if use_color {
            cfg.get("color.diff.meta")
                .and_then(|s| grit_lib::config::parse_color(&s).ok())
                .unwrap_or_else(|| "\x1b[31m".to_string())
        } else {
            String::new()
        };
        let reset = if use_color { "\x1b[m" } else { "" };
        Some(format!("{line_prefix}{red}|{reset}  "))
    } else {
        None
    };

    for node in nodes {
        let info = load_commit_info(repo, node.oid)?;
        graph.update(node.clone());

        loop {
            let (line, shown_commit_line) = graph.next_line();
            if shown_commit_line {
                if args.oneline || args.format.as_deref() == Some("oneline") {
                    let rendered = render_graph_commit_text(
                        &node,
                        &info,
                        args,
                        decorations.as_ref(),
                        abbrev_len,
                        &node.parents,
                    );
                    writeln!(out, "{line_prefix}{line}{rendered}")?;
                } else {
                    let mut body_buf = Vec::new();
                    format_commit(
                        &mut body_buf,
                        &node.oid,
                        &info,
                        args,
                        decorations.as_ref(),
                        use_color,
                        &mut notes_cache,
                        &repo.odb,
                        Some(node.parents.as_slice()),
                        false,
                        None,
                        None,
                        None,
                    )?;
                    let body = String::from_utf8(body_buf)
                        .map_err(|e| anyhow::anyhow!("invalid UTF-8 in log output: {e}"))?;
                    write_graph_interleaved_commit_msg(
                        &mut out,
                        line_prefix,
                        &line,
                        &mut graph,
                        &body,
                    )?;
                }
                break;
            }
            writeln!(out, "{line_prefix}{line}")?;
        }

        while !graph.is_commit_finished() {
            let (line, _) = graph.next_line();
            writeln!(out, "{line_prefix}{line}")?;
        }

        if show_commit_body {
            if !args.stat.is_empty() {
                writeln!(out)?;
            }
            write_commit_diff(
                &mut out,
                repo,
                &node.oid,
                &info,
                args,
                &combined_pathspecs,
                graph_stat_prefix.as_deref(),
                decorations.as_ref(),
                use_color,
                &mut notes_cache,
                patch_context,
            )?;
        }
    }

    Ok(())
}

fn simplify_merges_for_graph(repo: &Repository, commits: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let selected: HashSet<ObjectId> = commits.iter().copied().collect();
    let mut out = Vec::new();
    for oid in commits {
        let raw_parents = load_raw_parents(repo, *oid)?;
        let mut direct = load_raw_parents(repo, *oid)?;
        direct.retain(|p| selected.contains(p));
        if raw_parents.len() > 1 && direct.len() <= 1 {
            continue;
        }
        if direct.len() <= 1 {
            out.push(*oid);
            continue;
        }

        let mut simplified = graph_simplify_parent_list(repo, &selected, &direct)?;
        simplified.sort_unstable();
        simplified.dedup();
        if simplified.len() > 1 {
            out.push(*oid);
        }
    }
    Ok(out)
}

fn simplify_by_decoration_for_graph(
    repo: &Repository,
    commits: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let decorations = collect_decorations(repo, false)?;
    let decorated: HashSet<ObjectId> = decorations
        .keys()
        .filter_map(|hex| hex.parse::<ObjectId>().ok())
        .collect();

    let mut out = Vec::new();
    for oid in commits {
        if decorated.contains(oid) {
            out.push(*oid);
            continue;
        }
        let parents = load_raw_parents(repo, *oid)?;
        if parents.len() > 1 {
            out.push(*oid);
        }
    }
    Ok(out)
}

fn graph_simplify_parent_list(
    repo: &Repository,
    selected: &HashSet<ObjectId>,
    parents: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let mut out = Vec::new();
    for parent in parents {
        if parent_reachable_via_others(repo, selected, *parent, parents)? {
            continue;
        }
        out.push(*parent);
    }
    Ok(out)
}

fn parent_reachable_via_others(
    repo: &Repository,
    selected: &HashSet<ObjectId>,
    target: ObjectId,
    parents: &[ObjectId],
) -> Result<bool> {
    for parent in parents {
        if *parent == target {
            continue;
        }
        if graph_reaches(repo, selected, *parent, target)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn graph_reaches(
    repo: &Repository,
    selected: &HashSet<ObjectId>,
    start: ObjectId,
    target: ObjectId,
) -> Result<bool> {
    let mut stack = vec![start];
    let mut seen = HashSet::new();
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        if oid == target {
            return Ok(true);
        }
        let mut parents = load_raw_parents(repo, oid)?;
        parents.retain(|p| selected.contains(p));
        stack.extend(parents);
    }
    Ok(false)
}

fn load_raw_parents(repo: &Repository, oid: ObjectId) -> Result<Vec<ObjectId>> {
    let object = repo.odb.read(&oid)?;
    let commit = parse_commit(&object.data)?;
    Ok(commit.parents)
}

fn visible_parents_for_graph(
    repo: &Repository,
    oid: ObjectId,
    included: &HashSet<ObjectId>,
    first_parent_only: bool,
    first_parent_through_omitted: bool,
    simplify_merge_parents: bool,
) -> Result<Vec<ObjectId>> {
    let mut direct = load_raw_parents(repo, oid)?;
    if first_parent_only && direct.len() > 1 {
        direct.truncate(1);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for parent in direct {
        collect_visible_parent_for_graph(
            repo,
            parent,
            included,
            first_parent_only,
            first_parent_through_omitted,
            &mut seen,
            &mut out,
        )?;
    }
    if simplify_merge_parents && out.len() > 1 {
        let simplified = graph_simplify_parent_list(repo, included, &out)?;
        let keep: HashSet<ObjectId> = simplified.into_iter().collect();
        out.retain(|parent| keep.contains(parent));
    }
    let mut dedup = HashSet::new();
    out.retain(|parent| dedup.insert(*parent));
    Ok(out)
}

fn collect_visible_parent_for_graph(
    repo: &Repository,
    candidate: ObjectId,
    included: &HashSet<ObjectId>,
    first_parent_only: bool,
    first_parent_through_omitted: bool,
    seen: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
) -> Result<()> {
    if !seen.insert(candidate) {
        return Ok(());
    }
    if included.contains(&candidate) {
        out.push(candidate);
        return Ok(());
    }
    let mut parents = load_raw_parents(repo, candidate)?;
    if parents.is_empty() {
        return Ok(());
    }
    let fp_chain = first_parent_only || first_parent_through_omitted;
    if fp_chain && parents.len() > 1 {
        parents.truncate(1);
    }
    for parent in parents {
        collect_visible_parent_for_graph(
            repo,
            parent,
            included,
            first_parent_only,
            first_parent_through_omitted,
            seen,
            out,
        )?;
    }
    Ok(())
}

fn first_parent_of_commit(repo: &Repository, oid: ObjectId) -> Result<Option<ObjectId>> {
    let parents = load_raw_parents(repo, oid)?;
    Ok(parents.first().copied())
}

fn first_parent_anchor_in_set(
    repo: &Repository,
    start: ObjectId,
    anchors: &HashSet<ObjectId>,
) -> Result<Option<ObjectId>> {
    let mut seen = HashSet::new();
    let mut cursor = Some(start);
    while let Some(oid) = cursor {
        if !seen.insert(oid) {
            break;
        }
        if anchors.contains(&oid) {
            return Ok(Some(oid));
        }
        cursor = first_parent_of_commit(repo, oid)?;
    }
    Ok(None)
}

fn reorder_path_limited_graph_commits(
    repo: &Repository,
    commits: &[ObjectId],
    first_parent_only: bool,
) -> Result<Vec<ObjectId>> {
    if commits.is_empty() {
        return Ok(Vec::new());
    }

    let included: HashSet<ObjectId> = commits.iter().copied().collect();
    let mut chain = Vec::new();
    let mut chain_seen = HashSet::new();
    let mut cursor = Some(commits[0]);
    while let Some(oid) = cursor {
        if !included.contains(&oid) || !chain_seen.insert(oid) {
            break;
        }
        chain.push(oid);
        let visible =
            visible_parents_for_graph(repo, oid, &included, first_parent_only, false, false)?;
        cursor = visible.first().copied();
    }

    let chain_set: HashSet<ObjectId> = chain.iter().copied().collect();
    let mut grouped: HashMap<Option<ObjectId>, Vec<ObjectId>> = HashMap::new();
    for oid in commits {
        if chain_set.contains(oid) {
            continue;
        }
        let anchor = first_parent_anchor_in_set(repo, *oid, &chain_set)?;
        grouped.entry(anchor).or_default().push(*oid);
    }

    let mut ordered = Vec::new();
    for chain_oid in chain {
        if let Some(group) = grouped.remove(&Some(chain_oid)) {
            ordered.extend(group);
        }
        ordered.push(chain_oid);
    }
    if let Some(group) = grouped.remove(&None) {
        ordered.extend(group);
    }
    for (_anchor, group) in grouped {
        ordered.extend(group);
    }
    Ok(ordered)
}

fn expand_sparse_path_limited_graph_history(
    repo: &Repository,
    commits: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    if commits.is_empty() {
        return Ok(Vec::new());
    }

    let mut expanded = Vec::new();
    let mut seen = HashSet::new();
    let mut push_unique = |oid: ObjectId, out: &mut Vec<ObjectId>| {
        if seen.insert(oid) {
            out.push(oid);
        }
    };

    for window in commits.windows(2) {
        let from = window[0];
        let to = window[1];
        push_unique(from, &mut expanded);

        let mut cursor = first_parent_of_commit(repo, from)?;
        let mut chain = Vec::new();
        let mut found_target = false;
        let mut local_seen = HashSet::new();
        while let Some(oid) = cursor {
            if !local_seen.insert(oid) {
                break;
            }
            if oid == to {
                found_target = true;
                break;
            }
            chain.push(oid);
            cursor = first_parent_of_commit(repo, oid)?;
        }
        if found_target {
            for oid in chain {
                push_unique(oid, &mut expanded);
            }
        }
    }

    if let Some(&last) = commits.last() {
        push_unique(last, &mut expanded);
        let mut cursor = first_parent_of_commit(repo, last)?;
        let mut tail_seen = HashSet::new();
        while let Some(oid) = cursor {
            if !tail_seen.insert(oid) {
                break;
            }
            push_unique(oid, &mut expanded);
            cursor = first_parent_of_commit(repo, oid)?;
        }
    }

    Ok(expanded)
}

fn order_boundary_commits_for_graph(
    repo: &Repository,
    boundaries: &[ObjectId],
    first_included: Option<ObjectId>,
) -> Result<Vec<ObjectId>> {
    if boundaries.is_empty() {
        return Ok(Vec::new());
    }

    let boundary_set: HashSet<ObjectId> = boundaries.iter().copied().collect();
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();

    if let Some(start) = first_included {
        let mut cursor = first_parent_of_commit(repo, start)?;
        while let Some(oid) = cursor {
            if !seen.insert(oid) {
                break;
            }
            if boundary_set.contains(&oid) {
                ordered.push(oid);
            }
            cursor = first_parent_of_commit(repo, oid)?;
        }
    }

    for oid in boundaries {
        if seen.insert(*oid) {
            ordered.push(*oid);
        }
    }

    Ok(ordered)
}

fn load_commit_info(repo: &Repository, oid: ObjectId) -> Result<CommitInfo> {
    let obj = repo.odb.read(&oid)?;
    let commit = parse_commit(&obj.data)?;
    Ok(CommitInfo {
        tree: commit.tree,
        parents: commit.parents,
        author: commit.author,
        committer: commit.committer,
        message: commit.message,
    })
}

fn render_graph_commit_text(
    node: &GraphCommitNode,
    info: &CommitInfo,
    args: &Args,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
    abbrev_len: usize,
    parent_line: &[ObjectId],
) -> String {
    let hex = node.oid.to_hex();
    if args.oneline || args.format.as_deref() == Some("oneline") {
        let first_line = info.message.lines().next().unwrap_or("");
        let dec = format_decoration(&hex, decorations);
        return format!(
            "{}{} {}",
            &hex[..abbrev_len.min(hex.len())],
            dec,
            first_line
        );
    }

    if let Some(fmt) = args.format.as_deref() {
        if fmt.starts_with("format:") || fmt.starts_with("tformat:") {
            let template = if let Some(t) = fmt.strip_prefix("format:") {
                t
            } else if let Some(t) = fmt.strip_prefix("tformat:") {
                t
            } else {
                fmt
            };
            return apply_format_string(
                template,
                &node.oid,
                info,
                decorations,
                args.date.as_deref(),
                abbrev_len,
                false,
                None,
                parent_line,
                None,
            );
        }
        if fmt.contains('%') {
            return apply_format_string(
                fmt,
                &node.oid,
                info,
                decorations,
                args.date.as_deref(),
                abbrev_len,
                false,
                None,
                parent_line,
                None,
            );
        }
    }

    info.message.lines().next().unwrap_or("").to_owned()
}

#[derive(Clone, Debug)]
struct GraphCommitNode {
    oid: ObjectId,
    parents: Vec<ObjectId>,
    is_boundary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphState {
    Padding,
    Skip,
    PreCommit,
    Commit,
    PostMerge,
    Collapsing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GraphColumn {
    oid: ObjectId,
}

#[derive(Debug)]
struct AsciiGraph {
    current: Option<GraphCommitNode>,
    num_parents: usize,
    width: usize,
    expansion_row: usize,
    state: GraphState,
    prev_state: GraphState,
    commit_index: usize,
    prev_commit_index: usize,
    merge_layout: isize,
    edges_added: isize,
    prev_edges_added: isize,
    num_columns: usize,
    num_new_columns: usize,
    mapping_size: usize,
    columns: Vec<GraphColumn>,
    new_columns: Vec<GraphColumn>,
    mapping: Vec<isize>,
    old_mapping: Vec<isize>,
}

impl AsciiGraph {
    fn new() -> Self {
        Self {
            current: None,
            num_parents: 0,
            width: 0,
            expansion_row: 0,
            state: GraphState::Padding,
            prev_state: GraphState::Padding,
            commit_index: 0,
            prev_commit_index: 0,
            merge_layout: 0,
            edges_added: 0,
            prev_edges_added: 0,
            num_columns: 0,
            num_new_columns: 0,
            mapping_size: 0,
            columns: Vec::new(),
            new_columns: Vec::new(),
            mapping: Vec::new(),
            old_mapping: Vec::new(),
        }
    }

    fn update(&mut self, commit: GraphCommitNode) {
        self.current = Some(commit);
        self.num_parents = self.current.as_ref().map_or(0, |c| c.parents.len());
        self.prev_commit_index = self.commit_index;
        self.update_columns();
        self.expansion_row = 0;
        if self.state != GraphState::Padding {
            self.state = GraphState::Skip;
        } else if self.needs_pre_commit_line() {
            self.state = GraphState::PreCommit;
        } else {
            self.state = GraphState::Commit;
        }
    }

    fn is_commit_finished(&self) -> bool {
        self.state == GraphState::Padding
    }

    fn next_line(&mut self) -> (String, bool) {
        if self.current.is_none() {
            return (String::new(), false);
        }
        let mut line = String::new();
        let shown_commit_line = match self.state {
            GraphState::Padding => {
                self.output_padding_line(&mut line);
                false
            }
            GraphState::Skip => {
                line.push_str("...");
                if self.needs_pre_commit_line() {
                    self.update_state(GraphState::PreCommit);
                } else {
                    self.update_state(GraphState::Commit);
                }
                false
            }
            GraphState::PreCommit => {
                self.output_pre_commit_line(&mut line);
                false
            }
            GraphState::Commit => {
                self.output_commit_line(&mut line);
                true
            }
            GraphState::PostMerge => {
                self.output_post_merge_line(&mut line);
                false
            }
            GraphState::Collapsing => {
                self.output_collapsing_line(&mut line);
                false
            }
        };

        let pad_width = self.width;
        if line.len() < pad_width {
            line.push_str(&" ".repeat(pad_width - line.len()));
        }
        (line, shown_commit_line)
    }

    fn update_state(&mut self, next: GraphState) {
        self.prev_state = self.state;
        self.state = next;
    }

    fn ensure_vec_sizes(&mut self, needed_columns: usize) {
        let placeholder = match self.current.as_ref() {
            Some(current) => current.oid,
            None => return,
        };
        if self.columns.len() < needed_columns {
            self.columns
                .resize(needed_columns, GraphColumn { oid: placeholder });
        }
        if self.new_columns.len() < needed_columns {
            self.new_columns
                .resize(needed_columns, GraphColumn { oid: placeholder });
        }
        let map_len = needed_columns.saturating_mul(2);
        if self.mapping.len() < map_len {
            self.mapping.resize(map_len, -1);
        }
        if self.old_mapping.len() < map_len {
            self.old_mapping.resize(map_len, -1);
        }
    }

    fn find_new_column_by_commit(&self, oid: ObjectId) -> Option<usize> {
        (0..self.num_new_columns).find(|&i| self.new_columns[i].oid == oid)
    }

    fn insert_into_new_columns(&mut self, oid: ObjectId, idx: isize) {
        let mut i = self.find_new_column_by_commit(oid).unwrap_or_else(|| {
            let pos = self.num_new_columns;
            self.new_columns[pos] = GraphColumn { oid };
            self.num_new_columns += 1;
            pos
        });

        let mapping_idx: usize;
        if self.num_parents > 1 && idx > -1 && self.merge_layout == -1 {
            let dist = idx - i as isize;
            let shift = if dist > 1 { (2 * dist) - 3 } else { 1 };
            self.merge_layout = if dist > 0 { 0 } else { 1 };
            self.edges_added = self.num_parents as isize + self.merge_layout - 2;
            mapping_idx = (self.width as isize + (self.merge_layout - 1) * shift).max(0) as usize;
            self.width = self
                .width
                .saturating_add((2 * self.merge_layout.max(0)) as usize);
        } else if self.edges_added > 0
            && self.width >= 2
            && self.mapping.get(self.width - 2).copied() == Some(i as isize)
        {
            mapping_idx = self.width - 2;
            self.edges_added = -1;
        } else {
            mapping_idx = self.width;
            self.width = self.width.saturating_add(2);
        }

        if mapping_idx >= self.mapping.len() {
            self.mapping.resize(mapping_idx + 1, -1);
        }
        self.mapping[mapping_idx] = i as isize;
        // Keep i mutable use explicit to satisfy clippy about needless mut in closure capture.
        i = i.saturating_add(0);
        let _ = i;
    }

    fn update_columns(&mut self) {
        std::mem::swap(&mut self.columns, &mut self.new_columns);
        self.num_columns = self.num_new_columns;
        self.num_new_columns = 0;

        let max_new_columns = self.num_columns.saturating_add(self.num_parents.max(1));
        self.ensure_vec_sizes(max_new_columns);
        self.mapping_size = max_new_columns.saturating_mul(2);
        for i in 0..self.mapping_size {
            self.mapping[i] = -1;
        }

        self.width = 0;
        self.prev_edges_added = self.edges_added;
        self.edges_added = 0;

        let current_oid = match self.current.as_ref() {
            Some(c) => c.oid,
            None => return,
        };

        let mut seen_this = false;
        let mut is_commit_in_columns = true;
        for i in 0..=self.num_columns {
            let col_oid = if i == self.num_columns {
                if seen_this {
                    break;
                }
                is_commit_in_columns = false;
                current_oid
            } else {
                self.columns[i].oid
            };

            if col_oid == current_oid {
                seen_this = true;
                self.commit_index = i;
                self.merge_layout = -1;
                let parents = self
                    .current
                    .as_ref()
                    .map(|c| c.parents.clone())
                    .unwrap_or_default();
                for parent in parents {
                    let idx = i as isize;
                    self.insert_into_new_columns(parent, idx);
                }
                if self.num_parents == 0 {
                    self.width = self.width.saturating_add(2);
                } else if !is_commit_in_columns && self.num_parents > 1 {
                    // Keep width progression stable for detached columns.
                    self.width = self.width.max((self.num_new_columns + 1) * 2);
                }
            } else {
                self.insert_into_new_columns(col_oid, -1);
            }
        }

        while self.mapping_size > 1 && self.mapping[self.mapping_size - 1] < 0 {
            self.mapping_size -= 1;
        }
    }

    fn num_dashed_parents(&self) -> isize {
        self.num_parents as isize + self.merge_layout - 3
    }

    fn num_expansion_rows(&self) -> usize {
        self.num_dashed_parents().max(0) as usize * 2
    }

    fn needs_pre_commit_line(&self) -> bool {
        self.num_parents >= 3
            && self.commit_index < self.num_columns.saturating_sub(1)
            && self.expansion_row < self.num_expansion_rows()
    }

    fn is_mapping_correct(&self) -> bool {
        for i in 0..self.mapping_size {
            let target = self.mapping[i];
            if target < 0 {
                continue;
            }
            if target as usize == i / 2 {
                continue;
            }
            return false;
        }
        true
    }

    fn output_padding_line(&self, line: &mut String) {
        for i in 0..self.num_new_columns {
            let _ = i;
            line.push('|');
            line.push(' ');
        }
    }

    fn output_pre_commit_line(&mut self, line: &mut String) {
        let mut seen_this = false;
        let current_oid = match self.current.as_ref() {
            Some(c) => c.oid,
            None => return,
        };

        for i in 0..self.num_columns {
            let col_oid = self.columns[i].oid;
            if col_oid == current_oid {
                seen_this = true;
                line.push('|');
                line.push_str(&" ".repeat(self.expansion_row));
            } else if seen_this && self.expansion_row == 0 {
                if self.prev_state == GraphState::PostMerge && self.prev_commit_index < i {
                    line.push('\\');
                } else {
                    line.push('|');
                }
            } else if seen_this && self.expansion_row > 0 {
                line.push('\\');
            } else {
                line.push('|');
            }
            line.push(' ');
        }

        self.expansion_row += 1;
        if !self.needs_pre_commit_line() {
            self.update_state(GraphState::Commit);
        }
    }

    fn output_commit_char(&self) -> char {
        if self.current.as_ref().is_some_and(|c| c.is_boundary) {
            'o'
        } else {
            '*'
        }
    }

    fn draw_octopus_merge(&self, line: &mut String) {
        let dashed = self.num_dashed_parents().max(0) as usize;
        for i in 0..dashed {
            let map_idx = (self.commit_index + i + 2) * 2;
            let j = self.mapping.get(map_idx).copied().unwrap_or(-1);
            if j < 0 || j as usize >= self.num_new_columns {
                continue;
            }
            line.push('-');
            line.push(if i == dashed - 1 { '.' } else { '-' });
        }
    }

    fn output_commit_line(&mut self, line: &mut String) {
        let mut seen_this = false;
        let current_oid = match self.current.as_ref() {
            Some(c) => c.oid,
            None => return,
        };

        for i in 0..=self.num_columns {
            let col_oid = if i == self.num_columns {
                if seen_this {
                    break;
                }
                current_oid
            } else {
                self.columns[i].oid
            };

            if col_oid == current_oid {
                seen_this = true;
                line.push(self.output_commit_char());
                if self.num_parents > 2 {
                    self.draw_octopus_merge(line);
                }
            } else if seen_this && self.edges_added > 1 {
                line.push('\\');
            } else if seen_this && self.edges_added == 1 {
                if self.prev_state == GraphState::PostMerge
                    && self.prev_edges_added > 0
                    && self.prev_commit_index < i
                {
                    line.push('\\');
                } else {
                    line.push('|');
                }
            } else if self.prev_state == GraphState::Collapsing
                && (2 * i + 1) < self.old_mapping.len()
                && self.old_mapping[2 * i + 1] == i as isize
                && (2 * i) < self.mapping.len()
                && self.mapping[2 * i] < i as isize
            {
                line.push('/');
            } else {
                line.push('|');
            }
            line.push(' ');
        }

        if self.num_parents > 1 {
            self.update_state(GraphState::PostMerge);
        } else if self.is_mapping_correct() {
            self.update_state(GraphState::Padding);
        } else {
            self.update_state(GraphState::Collapsing);
        }
    }

    fn output_post_merge_line(&mut self, line: &mut String) {
        let merge_chars = ['/', '|', '\\'];
        let current = match self.current.as_ref() {
            Some(c) => c,
            None => return,
        };
        let first_parent = current.parents.first().copied();
        let mut parent_col_seen = false;
        let mut seen_this = false;

        for i in 0..=self.num_columns {
            let col_oid = if i == self.num_columns {
                if seen_this {
                    break;
                }
                current.oid
            } else {
                self.columns[i].oid
            };

            if col_oid == current.oid {
                seen_this = true;
                let mut idx = self.merge_layout.clamp(0, 2) as usize;
                for (j, parent) in current.parents.iter().enumerate() {
                    if self.find_new_column_by_commit(*parent).is_none() {
                        continue;
                    }
                    let c = merge_chars[idx.min(2)];
                    line.push(c);
                    if idx == 2 {
                        if self.edges_added > 0 || j < current.parents.len().saturating_sub(1) {
                            line.push(' ');
                        }
                    } else {
                        idx += 1;
                    }
                }
                if self.edges_added == 0 {
                    line.push(' ');
                }
            } else if seen_this {
                line.push(if self.edges_added > 0 { '\\' } else { '|' });
                line.push(' ');
            } else {
                line.push('|');
                if self.merge_layout != 0 || i != self.commit_index.saturating_sub(1) {
                    line.push(if parent_col_seen { '_' } else { ' ' });
                }
            }

            if first_parent.is_some_and(|p| p == col_oid) {
                parent_col_seen = true;
            }
        }

        if self.is_mapping_correct() {
            self.update_state(GraphState::Padding);
        } else {
            self.update_state(GraphState::Collapsing);
        }
    }

    fn output_collapsing_line(&mut self, line: &mut String) {
        std::mem::swap(&mut self.mapping, &mut self.old_mapping);
        for i in 0..self.mapping_size {
            self.mapping[i] = -1;
        }

        let mut used_horizontal = false;
        let mut horizontal_edge: isize = -1;
        let mut horizontal_target: isize = -1;

        for i in 0..self.mapping_size {
            let target = self.old_mapping[i];
            if target < 0 {
                continue;
            }
            if (target as usize) * 2 == i {
                self.mapping[i] = target;
            } else if i > 0 && self.mapping[i - 1] < 0 {
                self.mapping[i - 1] = target;
                if horizontal_edge == -1 {
                    horizontal_edge = i as isize;
                    horizontal_target = target;
                    let mut j = (target as usize).saturating_mul(2).saturating_add(3);
                    while j < i.saturating_sub(2) {
                        self.mapping[j] = target;
                        j += 2;
                    }
                }
            } else if i > 0 && self.mapping[i - 1] == target {
                continue;
            } else if i > 1 && self.mapping[i - 2] < 0 {
                self.mapping[i - 2] = target;
                if horizontal_edge == -1 {
                    horizontal_target = target;
                    horizontal_edge = i as isize - 1;
                    let mut j = (target as usize).saturating_mul(2).saturating_add(3);
                    while j < i.saturating_sub(2) {
                        self.mapping[j] = target;
                        j += 2;
                    }
                }
            }
        }

        for i in 0..self.mapping_size {
            self.old_mapping[i] = self.mapping[i];
        }
        if self.mapping_size > 0 && self.mapping[self.mapping_size - 1] < 0 {
            self.mapping_size -= 1;
        }

        for i in 0..self.mapping_size {
            let target = self.mapping[i];
            if target < 0 {
                line.push(' ');
            } else if (target as usize) * 2 == i {
                line.push('|');
            } else if target == horizontal_target && i as isize != horizontal_edge - 1 {
                if i != (target as usize).saturating_mul(2).saturating_add(3) {
                    self.mapping[i] = -1;
                }
                used_horizontal = true;
                line.push('_');
            } else {
                if used_horizontal && (i as isize) < horizontal_edge {
                    self.mapping[i] = -1;
                }
                line.push('/');
            }
        }

        if self.is_mapping_correct() {
            self.update_state(GraphState::Padding);
        }
    }
}

fn load_bloom_walk_config(git_dir: &Path) -> (bool, bool, i32) {
    let cfg = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    let mut core_cg = cfg
        .get_bool("core.commitgraph")
        .and_then(|r| r.ok())
        .unwrap_or(true);
    if std::env::var("GIT_TEST_COMMIT_GRAPH").ok().as_deref() == Some("0") {
        core_cg = false;
    }
    let read_paths = cfg
        .get("commitgraph.readchangedpaths")
        .and_then(|v| grit_lib::config::parse_bool(&v).ok())
        .unwrap_or(true);
    let version = cfg
        .get("commitgraph.changedpathsversion")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(-1);
    (core_cg, read_paths, version)
}

struct BloomPerfGuard(BloomWalkStatsHandle);

impl Drop for BloomPerfGuard {
    fn drop(&mut self) {
        let Ok(path) = std::env::var("GIT_TRACE2_PERF") else {
            return;
        };
        if path.is_empty() {
            return;
        }
        let Ok(stats) = self.0.lock() else {
            return;
        };
        emit_bloom_perf_line(&stats, &path);
    }
}

fn emit_bloom_perf_line(stats: &BloomWalkStats, path: &str) {
    let now = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let total_secs = now.as_secs();
        let micros = now.subsec_micros();
        let secs_in_day = total_secs % 86400;
        let hours = secs_in_day / 3600;
        let mins = (secs_in_day % 3600) / 60;
        let secs = secs_in_day % 60;
        format!("{:02}:{:02}:{:02}.{:06}", hours, mins, secs, micros)
    };
    let data = format!(
        "statistics:{{\"filter_not_present\":{},\"maybe\":{},\"definitely_not\":{},\"false_positive\":{}}}",
        stats.filter_not_present, stats.maybe, stats.definitely_not, stats.false_positive
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::Path::new(path))
    {
        let _ = writeln!(
            file,
            "{} grit:0  | d0 | main                     | {:<12} |     |           |           |              | {}",
            now, "data_json", data
        );
    }
}

/// Run the `log` command.
pub fn run(mut args: Args) -> Result<()> {
    let saw_bare_l = args.line_range.iter().any(|s| s.is_empty());
    args.line_range.retain(|s| !s.is_empty());
    if saw_bare_l && args.line_range.is_empty() {
        anyhow::bail!("switch `L' requires a value");
    }

    for val in &args.stat {
        if val.is_empty() {
            continue;
        }
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

    let repo = Repository::discover(None).context("not a git repository")?;
    if grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir)) {
        for p in &mut args.pathspecs {
            *p = grit_lib::unicode_normalization::precompose_utf8_path(p).into_owned();
        }
    }
    normalize_log_merge_diff_args(&mut args, &repo.git_dir)?;
    validate_log_pickaxe_options(&repo, &args)?;
    let patch_context = if let Some(u) = args.unified {
        u
    } else {
        let cfg = ConfigSet::load(Some(&repo.git_dir), true).context("loading git config")?;
        grit_lib::config::resolve_diff_context_lines(&cfg)
            .map_err(|m| anyhow::anyhow!(m))?
            .unwrap_or(3)
    };
    if !args.line_range.is_empty() {
        return run_line_log(&repo, args, patch_context);
    }
    if args
        .revisions
        .iter()
        .any(|r| r != "--" && is_symmetric_diff(r))
    {
        return run_symmetric_log(&repo, &args, patch_context);
    }
    validate_pathspec_scope(&repo, &args.pathspecs)?;
    let mut implied_pathspecs: Vec<String> = Vec::new();

    let use_color = log_resolve_stdout_color(&args, &repo.git_dir);

    if !args.walk_reflogs && !args.grep_reflog_patterns.is_empty() {
        anyhow::bail!("--grep-reflog can only be used with -g");
    }

    let mut author_res: Vec<Regex> = Vec::new();
    for p in &args.authors {
        let pat = if args.fixed_strings {
            regex::escape(p)
        } else {
            p.clone()
        };
        let re = RegexBuilder::new(&pat)
            .case_insensitive(true)
            .build()
            .with_context(|| format!("invalid --author regex: {p}"))?;
        author_res.push(re);
    }
    let mut committer_res: Vec<Regex> = Vec::new();
    for p in &args.committers {
        let pat = if args.fixed_strings {
            regex::escape(p)
        } else {
            p.clone()
        };
        let re = RegexBuilder::new(&pat)
            .case_insensitive(true)
            .build()
            .with_context(|| format!("invalid --committer regex: {p}"))?;
        committer_res.push(re);
    }
    let mut grep_res: Vec<Regex> = Vec::new();
    for p in &args.grep_patterns {
        let pat = if args.fixed_strings {
            regex::escape(p)
        } else {
            p.replace(r"\|", "|")
        };
        let mut b = RegexBuilder::new(&pat);
        // Commit message `--grep` / `--grep-reflog` match case-insensitively (t8290 and
        // typical user expectation). `-i` / `--regexp-ignore-case` is a no-op for these
        // patterns but remains valid for CLI compatibility.
        b.case_insensitive(true);
        let re = b
            .build()
            .with_context(|| format!("invalid --grep regex: {p}"))?;
        grep_res.push(re);
    }
    let mut grep_reflog_res: Vec<Regex> = Vec::new();
    for p in &args.grep_reflog_patterns {
        let pat = if args.fixed_strings {
            regex::escape(p)
        } else {
            p.replace(r"\|", "|")
        };
        let mut b = RegexBuilder::new(&pat);
        b.case_insensitive(true);
        let re = b
            .build()
            .with_context(|| format!("invalid --grep-reflog regex: {p}"))?;
        grep_reflog_res.push(re);
    }

    // --no-graph overrides --graph
    if args.no_graph {
        args.graph = false;
    }

    // Detect conflicting flag combinations
    if args.graph {
        if args.reverse {
            anyhow::bail!("options '--reverse' and '--graph' cannot be used together");
        }
        if args.no_walk.is_some() {
            anyhow::bail!("options '--no-walk' and '--graph' cannot be used together");
        }
        if args.walk_reflogs {
            anyhow::bail!("options '--walk-reflogs' and '--graph' cannot be used together");
        }
        if args.show_linear_break.is_some() {
            anyhow::bail!("options '--show-linear-break' and '--graph' cannot be used together");
        }
    }

    // Resolve pretty format aliases from config
    if let Some(ref fmt) = args.format {
        let resolved = resolve_pretty_alias_with_config(fmt, &repo);
        if resolved != *fmt {
            args.format = Some(resolved);
        }
    }

    // Handle -g / --walk-reflogs mode
    if args.walk_reflogs {
        return run_reflog_walk(
            &repo,
            &args,
            patch_context,
            &author_res,
            &committer_res,
            &grep_res,
            &grep_reflog_res,
        );
    }

    // Handle --no-walk: show given commits without walking parents
    if args.no_walk.is_some() {
        return run_no_walk(&repo, &args, patch_context);
    }

    if args.graph {
        return run_graph_log(&repo, &args, patch_context);
    }

    // Determine starting points and excluded commits.
    // Revisions prefixed with `^` (e.g. `^HEAD`) mean "exclude this and its
    // ancestors" — standard git revision range syntax.
    let (start_oids, exclude_oids) = if args.all {
        (collect_all_ref_oids(&repo.git_dir)?, Vec::new())
    } else if args.alternate_refs {
        (
            grit_lib::refs::collect_alternate_ref_oids(&repo.git_dir)
                .context("failed to collect alternate refs")?,
            Vec::new(),
        )
    } else if let Some(pat) = args.internal_remotes_pattern.as_deref() {
        let glob_pat = if pat.is_empty() {
            Cow::Borrowed("refs/remotes/*")
        } else {
            Cow::Owned(format!("refs/remotes/{pat}"))
        };
        let tips: Vec<ObjectId> = refs::list_refs_glob(&repo.git_dir, glob_pat.as_ref())
            .context("failed to list remote-tracking refs")?
            .into_iter()
            .map(|(_, oid)| oid)
            .collect();
        (tips, Vec::new())
    } else if args.revisions.is_empty() {
        if args.branches {
            let mut pairs: Vec<(ObjectId, i64)> = Vec::new();
            let mut seen: std::collections::HashSet<ObjectId> = std::collections::HashSet::new();
            for (_, oid) in grit_lib::refs::list_refs(&repo.git_dir, "refs/heads/")? {
                if seen.insert(oid) {
                    let obj = repo.odb.read(&oid)?;
                    let c = parse_commit(&obj.data)?;
                    let e = extract_epoch_from_ident(&c.committer);
                    pairs.push((oid, e));
                }
            }
            pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let oids: Vec<ObjectId> = pairs.into_iter().map(|(o, _)| o).collect();
            if oids.is_empty() {
                let head = resolve_head(&repo.git_dir)?;
                match head.oid() {
                    Some(oid) => (vec![*oid], Vec::new()),
                    // No commits yet: succeed with empty output (t8290; friendlier than Git's fatal).
                    None => (Vec::new(), Vec::new()),
                }
            } else {
                (oids, Vec::new())
            }
        } else {
            let head = resolve_head(&repo.git_dir)?;
            match head.oid() {
                Some(oid) => (vec![*oid], Vec::new()),
                None => (Vec::new(), Vec::new()),
            }
        }
    } else {
        let mut oids = Vec::new();
        let mut excludes = Vec::new();
        for rev in &args.revisions {
            if let Some(stripped) = rev.strip_prefix('^') {
                let oid = resolve_revision_as_commit(&repo, stripped)?;
                excludes.push(oid);
            } else if let Some((excl, tip)) = try_parse_double_dot_log_range(&repo, rev)? {
                excludes.push(excl);
                oids.push(tip);
            } else {
                match resolve_revision_as_commit(&repo, rev) {
                    Ok(oid) => oids.push(oid),
                    Err(_err) if is_likely_pathspec_during_rev_parse(rev) => {
                        implied_pathspecs.push(rev.clone());
                    }
                    Err(_err) => match resolve_revision_as_commit_after_precompose(&repo, rev) {
                        Ok(oid) => oids.push(oid),
                        Err(_err)
                            if grit_lib::precompose_config::effective_core_precomposeunicode(
                                Some(&repo.git_dir),
                            ) && grit_lib::unicode_normalization::has_non_ascii_utf8(rev) =>
                        {
                            implied_pathspecs.push(rev.clone());
                        }
                        Err(err) => return Err(err.into()),
                    },
                }
            }
        }
        // If only excludes are given with no positive refs, use HEAD
        if oids.is_empty() {
            let head = resolve_head(&repo.git_dir)?;
            if let Some(oid) = head.oid() {
                oids.push(*oid);
            }
        }
        (oids, excludes)
    };

    if !implied_pathspecs.is_empty() {
        validate_pathspec_scope(&repo, &implied_pathspecs)?;
    }

    // Pre-compute the set of OIDs reachable from excluded refs.
    let excluded_set = if exclude_oids.is_empty() {
        HashSet::new()
    } else {
        collect_reachable(&repo.odb, &exclude_oids)?
    };

    // Build source map for --source
    let source_map: std::collections::HashMap<ObjectId, String> = if args.source {
        if args.alternate_refs {
            build_alternate_source_map(&repo)?
        } else if let Some(pat) = args.internal_remotes_pattern.as_deref() {
            let glob_pat = if pat.is_empty() {
                Cow::Borrowed("refs/remotes/*")
            } else {
                Cow::Owned(format!("refs/remotes/{pat}"))
            };
            build_remote_tracking_source_map(
                &repo.odb,
                &repo.git_dir,
                glob_pat.as_ref(),
                args.first_parent,
            )?
        } else if args.all {
            build_source_map(&repo.odb, &repo.git_dir, args.first_parent)?
        } else {
            std::collections::HashMap::new()
        }
    } else {
        std::collections::HashMap::new()
    };

    let format_requires_decorations = args
        .format
        .as_deref()
        .map(|fmt| {
            let template = fmt
                .strip_prefix("format:")
                .or_else(|| fmt.strip_prefix("tformat:"))
                .unwrap_or(fmt);
            template.contains("%d") || template.contains("%D")
        })
        .unwrap_or(false);

    let (show_decorations, decorate_full) =
        resolve_decoration_display(&args, format_requires_decorations);
    // `--simplify-by-decoration` needs ref→OID mapping even when decorations are not shown
    // (`--oneline` does not imply `--decorate`). Use a separate map for display so we do not
    // print `(refs)` unless `--decorate` / `%d` requests it; OID keys match for full vs short maps.
    let decoration_map_for_display = if show_decorations {
        Some(collect_decorations(&repo, decorate_full)?)
    } else {
        None
    };
    let decoration_map_for_simplify_only = if args.simplify_by_decoration && !show_decorations {
        Some(collect_decorations(&repo, false)?)
    } else {
        None
    };
    let decoration_map_for_simplify = decoration_map_for_simplify_only
        .as_ref()
        .or(decoration_map_for_display.as_ref());

    // Walk commits
    let mut combined_pathspecs = args.pathspecs.clone();
    combined_pathspecs.extend(implied_pathspecs.iter().cloned());
    combined_pathspecs = resolve_effective_pathspecs(&repo, &combined_pathspecs)?;

    let effective_pathspecs = if args.follow {
        &[][..]
    } else {
        &combined_pathspecs[..]
    };

    let (core_commit_graph, cg_read_paths, cg_changed_ver) = load_bloom_walk_config(&repo.git_dir);
    let trace2_perf = std::env::var("GIT_TRACE2_PERF")
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let use_bloom = core_commit_graph
        && !combined_pathspecs.is_empty()
        && grit_lib::pathspec::pathspecs_allow_bloom(&combined_pathspecs)
        && !args.walk_reflogs;
    let bloom_read_changed_paths = cg_read_paths;
    let bloom_changed_paths_version = cg_changed_ver;
    let bloom_chain = if use_bloom {
        CommitGraphChain::load(&repo.git_dir.join("objects"))
    } else {
        None
    };
    let bloom_stats: Option<BloomWalkStatsHandle> = if trace2_perf && use_bloom {
        Some(Arc::new(Mutex::new(BloomWalkStats::default())))
    } else {
        None
    };
    let _bloom_perf_guard = bloom_stats.as_ref().map(|h| BloomPerfGuard(Arc::clone(h)));

    let bloom_pathspecs_for_walk: &[String] = if args.follow && use_bloom {
        &combined_pathspecs[..]
    } else {
        effective_pathspecs
    };
    let bloom_cwd_for_walk = if use_bloom {
        repo.bloom_pathspec_cwd()
    } else {
        None
    };

    let find_oid = if let Some(ref find_obj_rev) = args.find_object {
        Some(resolve_revision(&repo, find_obj_rev)?)
    } else {
        None
    };
    let find_object_tree_recursive = args.show_trees || args.recurse_trees;
    let since_str = args.since_as_filter.as_ref().or(args.since.as_ref());
    let since_threshold = since_str.and_then(|s| parse_date_to_epoch(s));
    let until_threshold = args.until.as_ref().and_then(|s| parse_date_to_epoch(s));
    let diff_filter_str = args.diff_filter.as_deref();

    let pickaxe_filter: Option<&Args> =
        if !args.remerge_diff && (args.pickaxe_grep.is_some() || args.pickaxe_string.is_some()) {
            Some(&args)
        } else {
            None
        };

    let use_streaming_log = !args.reverse && !(args.follow && !combined_pathspecs.is_empty());

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Detect format: (separator) vs tformat: (terminator) semantics
    let is_format_separator = args
        .format
        .as_deref()
        .map(|f| f.starts_with("format:"))
        .unwrap_or(false);

    let show_diff = args.patch
        || args.patch_u
        || !args.stat.is_empty()
        || args.name_only
        || args.name_status
        || args.raw
        || args.cc
        || args.merge_diff_c
        || args.remerge_diff
        || args.patch_with_stat;

    let mut notes_cache = NotesMapCache::new(&repo);
    let flush_each = out.is_terminal();

    if use_streaming_log {
        let mut iter = WalkCommitsIter::new(
            &repo.odb,
            &repo.git_dir,
            &start_oids,
            if args.follow { None } else { args.max_count }, // follow needs full walk for rename tracking
            args.skip,
            args.first_parent,
            &author_res,
            &committer_res,
            &grep_res,
            args.all_match,
            args.invert_grep,
            args.no_merges,
            args.merges,
            effective_pathspecs,
            &excluded_set,
            pickaxe_filter,
            bloom_chain.clone(),
            bloom_read_changed_paths,
            bloom_changed_paths_version,
            bloom_stats.clone(),
            bloom_pathspecs_for_walk,
            bloom_cwd_for_walk.clone(),
            args.author_date_order,
        );
        let mut shown = 0usize;
        let mut prev_had_notes = false;
        while let Some((oid, commit_data)) = iter.next_commit()? {
            if !commit_passes_post_walk_filters(
                &repo,
                &repo.odb,
                &oid,
                &commit_data,
                &args,
                diff_filter_str,
                find_oid,
                find_object_tree_recursive,
                decoration_map_for_simplify,
                since_threshold,
                until_threshold,
            )? {
                continue;
            }
            if is_format_separator && shown > 0 {
                if args.null_terminator {
                    write!(out, "\0")?;
                } else {
                    writeln!(out)?;
                }
            }
            let this_has_notes = commit_has_notes_to_show(&oid, &mut notes_cache, &args);
            if !is_format_separator && shown > 0 && prev_had_notes {
                writeln!(out)?;
            }
            let oneline_fmt = args.oneline || args.format.as_deref() == Some("oneline");
            if args.source && !oneline_fmt {
                if let Some(src) = source_map.get(&oid) {
                    write!(out, "{}\t", short_ref_for_source_display(src))?;
                }
            }
            let source_for_oneline = if args.source && oneline_fmt {
                source_map
                    .get(&oid)
                    .map(|full| short_ref_for_source_display(full))
            } else {
                None
            };
            format_commit(
                &mut out,
                &oid,
                &commit_data,
                &args,
                decoration_map_for_display.as_ref(),
                use_color,
                &mut notes_cache,
                &repo.odb,
                None,
                false,
                None,
                None,
                source_for_oneline,
            )?;

            if show_diff {
                write_commit_diff(
                    &mut out,
                    &repo,
                    &oid,
                    &commit_data,
                    &args,
                    effective_pathspecs,
                    None,
                    decoration_map_for_display.as_ref(),
                    use_color,
                    &mut notes_cache,
                    patch_context,
                )?;
            }
            if flush_each {
                out.flush()?;
            }
            shown += 1;
            prev_had_notes = this_has_notes;
        }
    } else {
        let commits = walk_commits(
            &repo.odb,
            &repo.git_dir,
            &start_oids,
            if args.follow { None } else { args.max_count }, // follow needs full walk for rename tracking
            args.skip,
            args.first_parent,
            &author_res,
            &committer_res,
            &grep_res,
            args.all_match,
            args.invert_grep,
            args.no_merges,
            args.merges,
            effective_pathspecs,
            &excluded_set,
            pickaxe_filter,
            bloom_chain,
            bloom_read_changed_paths,
            bloom_changed_paths_version,
            bloom_stats,
            bloom_pathspecs_for_walk,
            bloom_cwd_for_walk,
            args.author_date_order,
        )?;

        // Apply --follow: filter commits and track renames
        let commits = if args.follow && !combined_pathspecs.is_empty() {
            follow_filter(&repo.odb, commits, &combined_pathspecs[0], args.max_count)?
        } else {
            commits
        };

        // Apply --diff-filter
        let commits = if let Some(ref filter) = args.diff_filter {
            // Lowercase = exclude, uppercase = include
            let include_chars: Vec<char> = filter.chars().filter(|c| c.is_uppercase()).collect();
            let exclude_chars: Vec<char> = filter
                .chars()
                .filter(|c| c.is_lowercase())
                .map(|c| c.to_uppercase().next().unwrap_or(c))
                .collect();
            commits
                .into_iter()
                .filter(|(_oid, info)| {
                    if !include_chars.is_empty() {
                        commit_has_diff_status(&repo.odb, info, &include_chars).unwrap_or(true)
                    } else if !exclude_chars.is_empty() {
                        // Include if NOT in exclude list
                        commit_has_diff_status_not_in(&repo.odb, info, &exclude_chars)
                            .unwrap_or(true)
                    } else {
                        true
                    }
                })
                .collect::<Vec<_>>()
        } else {
            commits
        };

        // Apply --find-object: only show commits that introduce or remove the given object
        let commits = if let Some(ref find_obj_rev) = args.find_object {
            let find_oid_buf = resolve_revision(&repo, find_obj_rev)?;
            commits
                .into_iter()
                .filter(|(_oid, info)| {
                    commit_has_object(
                        &repo.odb,
                        info,
                        &find_oid_buf,
                        args.show_trees || args.recurse_trees,
                    )
                    .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        } else {
            commits
        };

        // Apply --simplify-by-decoration: only show commits with decorations
        let commits = if args.simplify_by_decoration {
            match decoration_map_for_simplify {
                Some(dec_map) => commits
                    .into_iter()
                    .filter(|(oid, _)| dec_map.contains_key(&oid.to_hex()))
                    .collect::<Vec<_>>(),
                None => commits,
            }
        } else {
            commits
        };

        // Apply --since-as-filter / --since
        let commits = {
            let since_str = args.since_as_filter.as_ref().or(args.since.as_ref());
            if let Some(s) = since_str {
                if let Some(threshold) = parse_date_to_epoch(s) {
                    commits
                        .into_iter()
                        .filter(|(_oid, info)| {
                            extract_epoch_from_ident(&info.committer) >= threshold
                        })
                        .collect::<Vec<_>>()
                } else {
                    commits
                }
            } else {
                commits
            }
        };
        // Apply --until
        let commits = if let Some(ref s) = args.until {
            if let Some(threshold) = parse_date_to_epoch(s) {
                commits
                    .into_iter()
                    .filter(|(_oid, info)| extract_epoch_from_ident(&info.committer) <= threshold)
                    .collect::<Vec<_>>()
            } else {
                commits
            }
        } else {
            commits
        };

        let commits = if args.reverse {
            commits.into_iter().rev().collect::<Vec<_>>()
        } else {
            commits
        };

        let mut prev_had_notes = false;
        for (i, (oid, commit_data)) in commits.iter().enumerate() {
            if is_format_separator && i > 0 {
                if args.null_terminator {
                    write!(out, "\0")?;
                } else {
                    writeln!(out)?;
                }
            }
            let this_has_notes = commit_has_notes_to_show(oid, &mut notes_cache, &args);
            if !is_format_separator && i > 0 && prev_had_notes {
                writeln!(out)?;
            }
            let oneline_fmt = args.oneline || args.format.as_deref() == Some("oneline");
            if args.source && !oneline_fmt {
                if let Some(src) = source_map.get(oid) {
                    write!(out, "{}\t", short_ref_for_source_display(src))?;
                }
            }
            let source_for_oneline = if args.source && oneline_fmt {
                source_map
                    .get(oid)
                    .map(|full| short_ref_for_source_display(full))
            } else {
                None
            };
            format_commit(
                &mut out,
                oid,
                commit_data,
                &args,
                decoration_map_for_display.as_ref(),
                use_color,
                &mut notes_cache,
                &repo.odb,
                None,
                false,
                None,
                None,
                source_for_oneline,
            )?;

            if show_diff {
                write_commit_diff(
                    &mut out,
                    &repo,
                    oid,
                    commit_data,
                    &args,
                    &combined_pathspecs,
                    None,
                    decoration_map_for_display.as_ref(),
                    use_color,
                    &mut notes_cache,
                    patch_context,
                )?;
            }
            prev_had_notes = this_has_notes;
        }
    }

    Ok(())
}

/// Ensure pathspecs are within the repository worktree scope.
///
/// Git rejects pathspecs that escape the worktree (e.g. `..`) as
/// "outside repository", and also rejects pathspecs provided while running in
/// an unqualified `.git` context.
fn validate_pathspec_scope(repo: &Repository, pathspecs: &[String]) -> Result<()> {
    if pathspecs.is_empty() {
        return Ok(());
    }

    let cwd = std::env::current_dir().context("resolving current directory")?;
    let Some(work_tree) = repo.work_tree.as_deref() else {
        // Bare repos: pathspecs limit history without resolving against a work tree (t0410).
        return Ok(());
    };

    let cwd_norm = normalize_path(&cwd);
    let work_tree_norm = normalize_path(work_tree);
    let git_dir_norm = normalize_path(&repo.git_dir);
    if cwd_norm.starts_with(&git_dir_norm) {
        anyhow::bail!("pathspec '{}' is outside repository", pathspecs[0]);
    }

    for pathspec in pathspecs {
        if pathspec.starts_with(':') {
            continue;
        }
        let as_path = Path::new(pathspec);
        let candidate = if as_path.is_absolute() {
            as_path.to_path_buf()
        } else {
            cwd_norm.join(as_path)
        };
        let candidate_norm = normalize_path(&candidate);
        if !candidate_norm.starts_with(&work_tree_norm) {
            anyhow::bail!("pathspec '{}' is outside repository", pathspec);
        }
    }

    Ok(())
}

/// Resolve pathspecs relative to current working directory inside the worktree.
///
/// This aligns pathspec matching semantics for commands invoked from
/// subdirectories, including magic forms like `:(icase)bar`.
fn resolve_effective_pathspecs(repo: &Repository, pathspecs: &[String]) -> Result<Vec<String>> {
    if pathspecs.is_empty() {
        return Ok(Vec::new());
    }
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(pathspecs.to_vec());
    };

    let cwd = std::env::current_dir().context("resolving current directory")?;
    let cwd_norm = normalize_path(&cwd);
    let work_tree_norm = normalize_path(work_tree);
    let cwd_rel = cwd_norm
        .strip_prefix(&work_tree_norm)
        .unwrap_or(Path::new(""));
    let cwd_prefix = if cwd_rel.as_os_str().is_empty() {
        String::new()
    } else {
        format!("{}/", cwd_rel.to_string_lossy())
    };

    let mut resolved = Vec::with_capacity(pathspecs.len());
    for spec in pathspecs {
        if spec.starts_with(":/") {
            resolved.push(spec.clone());
            continue;
        }

        if spec.starts_with(":(") {
            if let Some(resolved_magic) = crate::pathspec::resolve_magic_pathspec(spec, &cwd_prefix)
            {
                resolved.push(resolved_magic);
            } else {
                resolved.push(spec.clone());
            }
            continue;
        }

        if spec.starts_with(':') {
            resolved.push(spec.clone());
            continue;
        }

        let as_path = Path::new(spec);
        if as_path.is_absolute() {
            let candidate = normalize_path(as_path);
            if let Ok(rel) = candidate.strip_prefix(&work_tree_norm) {
                resolved.push(normalize_relative_path_str(&rel.to_string_lossy()));
            } else {
                resolved.push(spec.clone());
            }
            continue;
        }

        resolved.push(resolve_pathspec_tail_with_prefix(spec, &cwd_prefix));
    }

    Ok(resolved)
}

fn resolve_pathspec_tail_with_prefix(tail: &str, cwd_prefix: &str) -> String {
    if tail.is_empty() {
        return String::new();
    }
    if let Some(rooted) = tail.strip_prefix('/') {
        return normalize_relative_path_str(rooted);
    }
    if cwd_prefix.is_empty() {
        return normalize_relative_path_str(tail);
    }
    normalize_relative_path_str(&format!("{cwd_prefix}{tail}"))
}

fn normalize_relative_path_str(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(seg) => {
                parts.push(seg.to_string_lossy().to_string());
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

/// Normalize a path lexically by removing `.` and resolving `..`.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Run `--no-walk` mode: show the given commits without walking their parents.
pub fn run_no_walk(repo: &Repository, args: &Args, patch_context: usize) -> Result<()> {
    let mut oids = Vec::new();
    if args.revisions.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            oids.push(*oid);
        }
    } else {
        for rev in &args.revisions {
            let oid = resolve_revision_as_commit(repo, rev)?;
            oids.push(oid);
        }
    }

    let decorate_full = match &args.decorate {
        Some(Some(s)) if s == "full" => true,
        _ => false,
    };
    let decorations = if args.no_decorate {
        None
    } else if args.decorate.is_some() {
        // Explicitly requested decorations
        Some(collect_decorations(repo, decorate_full)?)
    } else {
        // Default: no decorations in no-walk mode (matches git behavior)
        None
    };

    let mut commits = Vec::new();
    for oid in oids {
        let obj = repo.read_replaced(&oid)?;
        let commit = parse_commit(&obj.data)?;
        let info = CommitInfo {
            tree: commit.tree,
            parents: commit.parents.clone(),
            author: commit.author.clone(),
            committer: commit.committer.clone(),
            message: commit.message.clone(),
        };
        commits.push((oid, info));
    }

    // Sort by committer timestamp descending (same as regular log)
    commits.sort_by(|a, b| {
        let ts_a = committer_unix_seconds_for_ordering(&a.1.committer);
        let ts_b = committer_unix_seconds_for_ordering(&b.1.committer);
        ts_b.cmp(&ts_a)
    });

    if args.reverse {
        commits.reverse();
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let is_format_separator = args
        .format
        .as_deref()
        .map(|f| f.starts_with("format:"))
        .unwrap_or(false);

    let show_diff = args.patch
        || args.patch_u
        || !args.stat.is_empty()
        || args.name_only
        || args.name_status
        || args.raw
        || args.cc
        || args.merge_diff_c
        || args.remerge_diff
        || args.patch_with_stat;

    let mut notes_cache = NotesMapCache::new(repo);

    let mut prev_had_notes = false;
    for (i, (oid, commit_data)) in commits.iter().enumerate() {
        if is_format_separator && i > 0 {
            writeln!(out)?;
        }
        let this_has_notes = commit_has_notes_to_show(oid, &mut notes_cache, args);
        if !is_format_separator && i > 0 && prev_had_notes {
            writeln!(out)?;
        }
        format_commit(
            &mut out,
            oid,
            commit_data,
            args,
            decorations.as_ref(),
            false,
            &mut notes_cache,
            &repo.odb,
            None,
            false,
            None,
            None,
            None,
        )?;
        if show_diff {
            write_commit_diff(
                &mut out,
                repo,
                oid,
                commit_data,
                args,
                &args.pathspecs,
                None,
                decorations.as_ref(),
                false,
                &mut notes_cache,
                patch_context,
            )?;
        }
        prev_had_notes = this_has_notes;
    }

    Ok(())
}

fn reflog_grep_matches(patterns: &[Regex], text: &str, all_match: bool, invert: bool) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let m = if all_match {
        patterns.iter().all(|re| re.is_match(text))
    } else {
        patterns.iter().any(|re| re.is_match(text))
    };
    if invert {
        !m
    } else {
        m
    }
}

/// Whether a reflog transition (`old_oid` → `new_oid`) touches any of `pathspecs`.
///
/// Used for `git reflog show -- <path>` / `log -g -- <path>`: Git filters reflog lines by whether
/// the tree diff for that specific reflog step matches the pathspec.
fn reflog_transition_touches_paths(
    odb: &Odb,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    pathspecs: &[String],
) -> Result<bool> {
    if pathspecs.is_empty() {
        return Ok(true);
    }
    let new_obj = odb.read(new_oid)?;
    let new_commit = parse_commit(&new_obj.data)?;
    let old_tree = if old_oid.is_zero() {
        None
    } else {
        let old_obj = match odb.read(old_oid) {
            Ok(o) => o,
            Err(_) => return Ok(false),
        };
        let old_commit = match parse_commit(&old_obj.data) {
            Ok(c) => c,
            Err(_) => return Ok(false),
        };
        Some(old_commit.tree)
    };
    let entries = diff_trees(odb, old_tree.as_ref(), Some(&new_commit.tree), "")?;
    Ok(entries.iter().any(|e| {
        let path = e.path();
        pathspecs.iter().any(|ps| path_matches(path, ps))
    }))
}

fn next_reflog_at_open_for_suffix(spec: &str, mut from: usize) -> Option<usize> {
    let b = spec.as_bytes();
    while let Some(rel) = spec[from..].find("@{") {
        let i = from + rel;
        if b.get(i + 2) == Some(&b'-') {
            let after_open = i + 2;
            let close = spec[after_open..].find('}').map(|j| after_open + j)?;
            from = close + 1;
            continue;
        }
        return Some(i);
    }
    None
}

fn reflog_entry_unix_ts(entry: &grit_lib::reflog::ReflogEntry) -> Option<i64> {
    let parts: Vec<&str> = entry.identity.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse().ok()
    } else {
        None
    }
}

fn reflog_entry_tz(entry: &grit_lib::reflog::ReflogEntry) -> &str {
    let parts: Vec<&str> = entry.identity.rsplitn(3, ' ').collect();
    parts.first().copied().unwrap_or("+0000")
}

fn format_reflog_selector_date(
    display_name: &str,
    entry: &grit_lib::reflog::ReflogEntry,
) -> String {
    if let Some(ts) = reflog_entry_unix_ts(entry) {
        let tz = reflog_entry_tz(entry);
        // `format_date_with_mode` parses Git signature tails via `parse_signature_tail`, which
        // requires the `Name <email>` prefix before `<unix> <tz>` (see grit-lib `ident.rs`).
        let pseudo = format!("x <x@x> {ts} {tz}");
        let date = format_date_with_mode(&pseudo, None);
        format!("{display_name}@{{{date}}}")
    } else {
        format!("{display_name}@{{0}}")
    }
}

/// Run the reflog walk mode (`log -g` / `log --walk-reflogs`).
fn run_reflog_walk(
    repo: &Repository,
    args: &Args,
    _patch_context: usize,
    author_res: &[Regex],
    committer_res: &[Regex],
    grep_res: &[Regex],
    grep_reflog_res: &[Regex],
) -> Result<()> {
    // Determine which ref to walk
    let refname = if args.revisions.is_empty() {
        "HEAD".to_string()
    } else {
        let r = &args.revisions[0];
        if let Ok(Some(w)) = reflog_walk_refname(repo, r) {
            w
        } else if r == "HEAD" || r.starts_with("refs/") {
            r.clone()
        } else if r.starts_with("@{") {
            // Resolve @{-N} to the previous branch name
            if let Some(n_str) = r.strip_prefix("@{").and_then(|s| s.strip_suffix('}')) {
                if let Some(stripped) = n_str.strip_prefix('-') {
                    if let Ok(_n) = stripped.parse::<usize>() {
                        if let Ok(branch) = grit_lib::refs::resolve_at_n_branch(&repo.git_dir, r) {
                            format!("refs/heads/{branch}")
                        } else {
                            r.clone()
                        }
                    } else {
                        r.clone()
                    }
                } else {
                    r.clone()
                }
            } else {
                r.clone()
            }
        } else {
            let candidate = format!("refs/heads/{r}");
            if grit_lib::refs::resolve_ref(&repo.git_dir, &candidate).is_ok() {
                candidate
            } else {
                r.clone()
            }
        }
    };

    // Use the original user-provided name for display (preserve full ref name if given)
    let orig_r_owned = args
        .revisions
        .first()
        .cloned()
        .unwrap_or_else(|| "HEAD".to_string());
    let orig_r = orig_r_owned.as_str();
    let display_name = if orig_r.starts_with("refs/") {
        orig_r
    } else if refname.starts_with("refs/heads/") {
        refname.strip_prefix("refs/heads/").unwrap_or(&refname)
    } else {
        &refname
    };

    let entries = read_reflog(&repo.git_dir, &refname).map_err(|e| anyhow::anyhow!("{e}"))?;

    if entries.is_empty() {
        return Ok(());
    }

    let effective_pathspecs = if args.pathspecs.is_empty() {
        Vec::new()
    } else {
        validate_pathspec_scope(repo, &args.pathspecs)?;
        resolve_effective_pathspecs(repo, &args.pathspecs)?
    };

    let nr = entries.len();
    let mut start_j: Option<usize> = None;
    let mut use_date_selector = false;

    let mut pos = 0usize;
    while let Some(at) = next_reflog_at_open_for_suffix(orig_r, pos) {
        let inner_start = at + 2;
        let Some(close) = orig_r[inner_start..].find('}').map(|j| inner_start + j) else {
            break;
        };
        let inner = &orig_r[inner_start..close];
        let inner_l = inner.to_ascii_lowercase();
        if inner_l == "u" || inner_l == "upstream" || inner_l == "push" {
            pos = close + 1;
            continue;
        }
        if let Ok(n) = inner.parse::<usize>() {
            let idx = nr.checked_sub(1 + n);
            start_j = Some(idx.unwrap_or(0));
            use_date_selector = false;
        } else if let Some(target_ts) = grit_lib::rev_parse::reflog_date_selector_timestamp(inner) {
            let mut picked = 0usize;
            for (j, e) in entries.iter().enumerate() {
                if let Some(ts) = reflog_entry_unix_ts(e) {
                    if ts <= target_ts {
                        picked = j;
                    }
                }
            }
            start_j = Some(picked);
            use_date_selector = true;
        } else {
            start_j = Some(nr.saturating_sub(1));
            use_date_selector = false;
        }
        pos = close + 1;
    }

    let start_j = start_j.unwrap_or(nr.saturating_sub(1));

    let max = args.max_count.unwrap_or(usize::MAX);
    let skip = args.skip.unwrap_or(0);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Detect format
    let is_format_separator = args
        .format
        .as_deref()
        .map(|f| f.starts_with("format:"))
        .unwrap_or(false);

    let mut shown = 0usize;
    let mut skipped = 0usize;

    for j in (0..=start_j).rev() {
        let entry = &entries[j];
        if shown >= max {
            break;
        }
        if skipped < skip {
            skipped += 1;
            continue;
        }

        // Read the commit object for this entry
        let commit_data = match repo.odb.read(&entry.new_oid) {
            Ok(obj) => match parse_commit(&obj.data) {
                Ok(c) => c,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        if !effective_pathspecs.is_empty()
            && !reflog_transition_touches_paths(
                &repo.odb,
                &entry.old_oid,
                &entry.new_oid,
                &effective_pathspecs,
            )?
        {
            continue;
        }

        let author_ok =
            author_res.is_empty() || author_res.iter().any(|re| re.is_match(&commit_data.author));
        if !author_ok {
            continue;
        }
        let committer_ok = committer_res.is_empty()
            || committer_res
                .iter()
                .any(|re| re.is_match(&commit_data.committer));
        if !committer_ok {
            continue;
        }
        let msg_ok = reflog_grep_matches(
            grep_res,
            &commit_data.message,
            args.all_match,
            args.invert_grep,
        );
        if !msg_ok {
            continue;
        }
        let reflog_ok = reflog_grep_matches(
            grep_reflog_res,
            &entry.message,
            args.all_match,
            args.invert_grep,
        );
        if !reflog_ok {
            continue;
        }

        let selector = if use_date_selector {
            format_reflog_selector_date(display_name, entry)
        } else {
            let idx_from_tip = nr - 1 - j;
            format!("{display_name}@{{{idx_from_tip}}}")
        };

        // NUL separator between entries for multi-line formats
        let is_oneline_fmt = args.format.as_deref() == Some("oneline") || args.oneline;
        if args.null_terminator && shown > 0 && !is_oneline_fmt {
            write!(out, "\0")?;
        }

        if let Some(ref fmt) = args.format {
            match fmt.as_str() {
                "oneline" => {
                    let abbrev = &entry.new_oid.to_hex()[..7];
                    let subject = commit_data.message.lines().next().unwrap_or("");
                    if args.null_terminator {
                        write!(out, "{} {}\0", abbrev, subject)?;
                    } else {
                        writeln!(out, "{} {}", abbrev, subject)?;
                    }
                }
                "short" => {
                    writeln!(out, "commit {}", entry.new_oid.to_hex())?;
                    let author_name = extract_name(&commit_data.author);
                    writeln!(out, "Author: {author_name}")?;
                    writeln!(out)?;
                    for line in commit_data.message.lines().take(1) {
                        writeln!(out, "    {line}")?;
                    }
                    writeln!(out)?;
                }
                "medium" => {
                    writeln!(out, "commit {}", entry.new_oid.to_hex())?;
                    writeln!(
                        out,
                        "Author: {}",
                        format_ident_for_header(&commit_data.author)
                    )?;
                    let date = format_date_for_header(&commit_data.author);
                    writeln!(out, "Date:   {}", date)?;
                    writeln!(out)?;
                    for line in commit_data.message.lines() {
                        writeln!(out, "    {}", line)?;
                    }
                    writeln!(out)?;
                }
                "full" => {
                    writeln!(out, "commit {}", entry.new_oid.to_hex())?;
                    writeln!(
                        out,
                        "Author: {}",
                        format_ident_for_header(&commit_data.author)
                    )?;
                    writeln!(
                        out,
                        "Commit: {}",
                        format_ident_for_header(&commit_data.committer)
                    )?;
                    writeln!(out)?;
                    for line in commit_data.message.lines() {
                        writeln!(out, "    {}", line)?;
                    }
                    writeln!(out)?;
                }
                "fuller" => {
                    writeln!(out, "commit {}", entry.new_oid.to_hex())?;
                    writeln!(
                        out,
                        "Author:     {}",
                        format_ident_for_header(&commit_data.author)
                    )?;
                    writeln!(
                        out,
                        "AuthorDate: {}",
                        format_date_for_header(&commit_data.author)
                    )?;
                    writeln!(
                        out,
                        "Commit:     {}",
                        format_ident_for_header(&commit_data.committer)
                    )?;
                    writeln!(
                        out,
                        "CommitDate: {}",
                        format_date_for_header(&commit_data.committer)
                    )?;
                    writeln!(out)?;
                    for line in commit_data.message.lines() {
                        writeln!(out, "    {}", line)?;
                    }
                    writeln!(out)?;
                }
                "email" => {
                    writeln!(
                        out,
                        "From {} Mon Sep 17 00:00:00 2001",
                        entry.new_oid.to_hex()
                    )?;
                    writeln!(
                        out,
                        "From: {}",
                        format_ident_for_header(&commit_data.author)
                    )?;
                    let date = format_date_for_header(&commit_data.author);
                    writeln!(out, "Date: {}", date)?;
                    let subject = commit_data.message.lines().next().unwrap_or("");
                    writeln!(out, "Subject: [PATCH] {}", subject)?;
                    writeln!(out)?;
                    for line in commit_data.message.lines() {
                        writeln!(out, "{}", line)?;
                    }
                    writeln!(out)?;
                }
                "raw" => {
                    writeln!(out, "commit {}", entry.new_oid.to_hex())?;
                    // Write raw commit data
                    writeln!(out, "tree {}", commit_data.tree.to_hex())?;
                    for parent in &commit_data.parents {
                        writeln!(out, "parent {}", parent.to_hex())?;
                    }
                    writeln!(out, "author {}", commit_data.author)?;
                    writeln!(out, "committer {}", commit_data.committer)?;
                    writeln!(out)?;
                    for line in commit_data.message.lines() {
                        writeln!(out, "    {}", line)?;
                    }
                    writeln!(out)?;
                }
                _ => {
                    let fmt_str = fmt
                        .strip_prefix("tformat:")
                        .or_else(|| fmt.strip_prefix("format:"))
                        .unwrap_or(fmt);
                    if is_format_separator && shown > 0 {
                        writeln!(out)?;
                    }
                    let line = apply_reflog_format_string(
                        fmt_str,
                        &entry.new_oid,
                        &commit_data,
                        &selector,
                        &entry.message,
                        &entry.identity,
                    );
                    writeln!(out, "{}", line)?;
                }
            }
        } else if args.oneline {
            let abbrev_len = args
                .abbrev
                .as_deref()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(7)
                .min(40);
            let full_hex = entry.new_oid.to_hex();
            let abbrev = &full_hex[..abbrev_len.min(full_hex.len())];
            if args.null_terminator {
                write!(out, "{} {}: {}\0", abbrev, selector, entry.message)?;
            } else {
                writeln!(out, "{} {}: {}", abbrev, selector, entry.message)?;
            }
        } else {
            // Full format with Reflog headers
            writeln!(out, "commit {}", entry.new_oid.to_hex())?;
            // Strip timestamp from identity for Reflog: line (git shows only Name <email>)
            let ident_display = if let Some(email_end) = entry.identity.rfind('>') {
                &entry.identity[..email_end + 1]
            } else {
                &entry.identity
            };
            writeln!(out, "Reflog: {} ({})", selector, ident_display)?;
            writeln!(out, "Reflog message: {}", entry.message)?;
            writeln!(
                out,
                "Author: {}",
                format_ident_for_header(&commit_data.author)
            )?;
            let date = format_date_for_header(&commit_data.author);
            writeln!(out, "Date:   {}", date)?;
            writeln!(out)?;
            for line in commit_data.message.lines() {
                writeln!(out, "    {}", line)?;
            }
        }
        shown += 1;
    }

    Ok(())
}

/// Apply format placeholders for reflog walk entries.
/// Supports %H, %h, %s, %gd, %gs, %gn, %ge, %an, %ae, %cn, %ce, %B, %b, %N, %n.
fn apply_reflog_format_string(
    fmt: &str,
    oid: &ObjectId,
    commit: &grit_lib::objects::CommitData,
    selector: &str,
    reflog_msg: &str,
    reflog_identity: &str,
) -> String {
    let hex = oid.to_hex();
    let short = &hex[..7.min(hex.len())];
    let subject = commit.message.lines().next().unwrap_or("");
    let body = extract_body(&commit.message);

    let reflog_name = extract_name(reflog_identity);
    let reflog_email = extract_email(reflog_identity);

    let mut result = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.peek() {
                Some('H') => {
                    chars.next();
                    result.push_str(&hex);
                }
                Some('h') => {
                    chars.next();
                    result.push_str(short);
                }
                Some('s') => {
                    chars.next();
                    result.push_str(subject);
                }
                Some('B') => {
                    chars.next();
                    // Entire commit message (matches Git `%B`). Parsed commits omit the final
                    // newline in memory; Git's `%B` still ends with `\n` when non-empty.
                    if !commit.message.is_empty() {
                        result.push_str(&commit.message);
                        if !commit.message.ends_with('\n') {
                            result.push('\n');
                        }
                    }
                }
                Some('b') => {
                    chars.next();
                    result.push_str(&body);
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('g') => {
                    chars.next();
                    match chars.peek() {
                        Some('d') => {
                            chars.next();
                            result.push_str(selector);
                        }
                        Some('s') => {
                            chars.next();
                            result.push_str(reflog_msg);
                        }
                        Some('n') => {
                            chars.next();
                            result.push_str(&reflog_name);
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&reflog_email);
                        }
                        _ => {
                            result.push_str("%g");
                        }
                    }
                }
                Some('a') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(&commit.author));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(&commit.author));
                        }
                        _ => {
                            result.push_str("%a");
                        }
                    }
                }
                Some('c') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(&commit.committer));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(&commit.committer));
                        }
                        _ => {
                            result.push_str("%c");
                        }
                    }
                }
                _ => {
                    result.push('%');
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Format ident for header display ("Name <email>").
fn format_ident_for_header(ident: &str) -> String {
    let name = extract_name(ident);
    let email = extract_email(ident);
    if email.is_empty() {
        name
    } else {
        format!("{name} <{email}>")
    }
}

/// Format date from ident for header display (`Date:` / `AuthorDate:`).
fn format_date_for_header(ident: &str) -> String {
    format_author_date_internal(ident, None, true)
}

/// Parsed commit with its OID.
struct CommitInfo {
    tree: ObjectId,
    parents: Vec<ObjectId>,
    author: String,
    committer: String,
    message: String,
}

/// Incremental commit walk matching [`walk_commits`] output order (date-ordered heap).
///
/// Used by `grit log` to print commits as they are discovered instead of buffering
/// the full history in a `Vec` first.
struct WalkCommitsIter<'a> {
    odb: &'a Odb,
    git_dir: &'a Path,
    pickaxe_args: Option<&'a Args>,
    bloom_chain: Option<CommitGraphChain>,
    bloom_read_changed_paths: bool,
    bloom_changed_paths_version: i32,
    bloom_stats: Option<BloomWalkStatsHandle>,
    bloom_pathspecs: &'a [String],
    bloom_cwd: Option<String>,
    author_date_order: bool,
    shallow_boundaries: HashSet<ObjectId>,
    visited: HashSet<ObjectId>,
    queue: std::collections::BinaryHeap<(i64, ObjectId)>,
    skipped: usize,
    skip_n: usize,
    max_count: Option<usize>,
    first_parent: bool,
    author_res: &'a [Regex],
    committer_res: &'a [Regex],
    grep_res: &'a [Regex],
    all_match_grep: bool,
    invert_grep: bool,
    no_merges: bool,
    merges_only: bool,
    pathspecs: &'a [String],
    accepted_count: usize,
}

impl<'a> WalkCommitsIter<'a> {
    fn new(
        odb: &'a Odb,
        git_dir: &'a Path,
        start: &[ObjectId],
        max_count: Option<usize>,
        skip: Option<usize>,
        first_parent: bool,
        author_res: &'a [Regex],
        committer_res: &'a [Regex],
        grep_res: &'a [Regex],
        all_match_grep: bool,
        invert_grep: bool,
        no_merges: bool,
        merges_only: bool,
        pathspecs: &'a [String],
        excluded: &HashSet<ObjectId>,
        pickaxe_args: Option<&'a Args>,
        bloom_chain: Option<CommitGraphChain>,
        bloom_read_changed_paths: bool,
        bloom_changed_paths_version: i32,
        bloom_stats: Option<BloomWalkStatsHandle>,
        bloom_pathspecs: &'a [String],
        bloom_cwd: Option<String>,
        author_date_order: bool,
    ) -> Self {
        let shallow_boundaries = load_shallow_boundaries(git_dir);
        let visited: HashSet<ObjectId> = excluded.clone();
        let mut queue: std::collections::BinaryHeap<(i64, ObjectId)> =
            std::collections::BinaryHeap::new();
        for oid in start {
            let ts = if author_date_order {
                read_author_timestamp(odb, oid)
            } else {
                read_commit_timestamp(odb, oid)
            };
            queue.push((ts, *oid));
        }
        Self {
            odb,
            git_dir,
            pickaxe_args,
            bloom_chain,
            bloom_read_changed_paths,
            bloom_changed_paths_version,
            bloom_stats,
            bloom_pathspecs,
            bloom_cwd,
            author_date_order,
            shallow_boundaries,
            visited,
            queue,
            skipped: 0,
            skip_n: skip.unwrap_or(0),
            max_count,
            first_parent,
            author_res,
            committer_res,
            grep_res,
            all_match_grep,
            invert_grep,
            no_merges,
            merges_only,
            pathspecs,
            accepted_count: 0,
        }
    }

    fn next_commit(&mut self) -> Result<Option<(ObjectId, CommitInfo)>> {
        if self.max_count == Some(0) {
            return Ok(None);
        }
        if let Some(max) = self.max_count {
            if self.accepted_count >= max {
                return Ok(None);
            }
        }
        while let Some((_ts, oid)) = self.queue.pop() {
            if !self.visited.insert(oid) {
                continue;
            }

            let obj = self.odb.read(&oid)?;
            if obj.kind == ObjectKind::Tag {
                let tag = parse_tag(&obj.data)?;
                let mut target = tag.object;
                loop {
                    let t_obj = self.odb.read(&target)?;
                    match t_obj.kind {
                        ObjectKind::Commit => {
                            let ts = if self.author_date_order {
                                read_author_timestamp(self.odb, &target)
                            } else {
                                read_commit_timestamp(self.odb, &target)
                            };
                            self.queue.push((ts, target));
                            break;
                        }
                        ObjectKind::Tag => {
                            let inner = parse_tag(&t_obj.data)?;
                            target = inner.object;
                        }
                        _ => break,
                    }
                }
                continue;
            }
            let commit = parse_commit(&obj.data)?;

            let info = CommitInfo {
                tree: commit.tree,
                parents: commit.parents.clone(),
                author: commit.author.clone(),
                committer: commit.committer.clone(),
                message: commit.message.clone(),
            };

            if !self.shallow_boundaries.contains(&oid) {
                if self.first_parent {
                    if let Some(parent) = commit.parents.first() {
                        if self.odb.read(parent).is_ok() {
                            let ts = if self.author_date_order {
                                read_author_timestamp(self.odb, parent)
                            } else {
                                read_commit_timestamp(self.odb, parent)
                            };
                            self.queue.push((ts, *parent));
                        }
                    }
                } else {
                    for parent in &commit.parents {
                        if !self.visited.contains(parent) && self.odb.read(parent).is_ok() {
                            let ts = if self.author_date_order {
                                read_author_timestamp(self.odb, parent)
                            } else {
                                read_commit_timestamp(self.odb, parent)
                            };
                            self.queue.push((ts, *parent));
                        }
                    }
                }
            }

            let is_merge = info.parents.len() > 1;
            if self.no_merges && is_merge {
                continue;
            }
            if self.merges_only && !is_merge {
                continue;
            }
            let author_ok = self.author_res.is_empty()
                || self.author_res.iter().any(|re| re.is_match(&info.author));
            if !author_ok {
                continue;
            }
            let committer_ok = self.committer_res.is_empty()
                || self
                    .committer_res
                    .iter()
                    .any(|re| re.is_match(&info.committer));
            if !committer_ok {
                continue;
            }
            let msg_ok = if self.grep_res.is_empty() {
                true
            } else {
                let m = if self.all_match_grep {
                    self.grep_res.iter().all(|re| re.is_match(&info.message))
                } else {
                    self.grep_res.iter().any(|re| re.is_match(&info.message))
                };
                if self.invert_grep {
                    !m
                } else {
                    m
                }
            };
            if !msg_ok {
                continue;
            }
            if !self.pathspecs.is_empty() {
                let touches = commit_touches_paths(
                    self.odb,
                    oid,
                    &info,
                    self.pathspecs,
                    self.bloom_chain.as_ref(),
                    self.bloom_read_changed_paths,
                    self.bloom_changed_paths_version,
                    self.bloom_stats.as_ref(),
                    self.bloom_pathspecs,
                    self.bloom_cwd.as_deref(),
                )?;
                if !touches {
                    continue;
                }
            }

            if let Some(pa) = self.pickaxe_args {
                if !commit_pickaxe_matches(self.git_dir, self.odb, &info, pa)? {
                    continue;
                }
            }

            if self.skipped < self.skip_n {
                self.skipped += 1;
            } else {
                self.accepted_count += 1;
                return Ok(Some((oid, info)));
            }
        }
        Ok(None)
    }
}

/// Walk the commit graph in reverse chronological order.
/// Collect all OIDs reachable from the given starting points.
fn collect_reachable(odb: &Odb, starts: &[ObjectId]) -> Result<HashSet<ObjectId>> {
    let mut visited = HashSet::new();
    let mut queue: Vec<ObjectId> = starts.to_vec();
    while let Some(oid) = queue.pop() {
        if !visited.insert(oid) {
            continue;
        }
        if let Ok(obj) = odb.read(&oid) {
            if let Ok(commit) = parse_commit(&obj.data) {
                for parent in &commit.parents {
                    if !visited.contains(parent) {
                        queue.push(*parent);
                    }
                }
            }
        }
    }
    Ok(visited)
}

fn walk_commits(
    odb: &Odb,
    git_dir: &Path,
    start: &[ObjectId],
    max_count: Option<usize>,
    skip: Option<usize>,
    first_parent: bool,
    author_res: &[Regex],
    committer_res: &[Regex],
    grep_res: &[Regex],
    all_match_grep: bool,
    invert_grep: bool,
    no_merges: bool,
    merges_only: bool,
    pathspecs: &[String],
    excluded: &HashSet<ObjectId>,
    pickaxe_args: Option<&Args>,
    bloom_chain: Option<CommitGraphChain>,
    bloom_read_changed_paths: bool,
    bloom_changed_paths_version: i32,
    bloom_stats: Option<BloomWalkStatsHandle>,
    bloom_pathspecs: &[String],
    bloom_cwd: Option<String>,
    author_date_order: bool,
) -> Result<Vec<(ObjectId, CommitInfo)>> {
    if max_count == Some(0) {
        return Ok(Vec::new());
    }
    let mut iter = WalkCommitsIter::new(
        odb,
        git_dir,
        start,
        max_count,
        skip,
        first_parent,
        author_res,
        committer_res,
        grep_res,
        all_match_grep,
        invert_grep,
        no_merges,
        merges_only,
        pathspecs,
        excluded,
        pickaxe_args,
        bloom_chain,
        bloom_read_changed_paths,
        bloom_changed_paths_version,
        bloom_stats,
        bloom_pathspecs,
        bloom_cwd,
        author_date_order,
    );
    let mut result = Vec::new();
    while let Some(c) = iter.next_commit()? {
        result.push(c);
    }
    Ok(result)
}

/// Check if a commit touches any of the given pathspecs by diffing against parents.
fn commit_touches_paths(
    odb: &Odb,
    commit_oid: ObjectId,
    info: &CommitInfo,
    pathspecs: &[String],
    bloom_chain: Option<&CommitGraphChain>,
    read_changed_paths: bool,
    changed_paths_version: i32,
    bloom_stats: Option<&BloomWalkStatsHandle>,
    bloom_specs: &[String],
    bloom_cwd: Option<&str>,
) -> Result<bool> {
    let bloom_keys = if bloom_specs.is_empty() {
        pathspecs
    } else {
        bloom_specs
    };

    if info.parents.is_empty() {
        let mut bloom_ret = BloomPrecheck::Inapplicable;
        if let Some(chain) = bloom_chain {
            if !bloom_keys.is_empty() {
                bloom_ret = chain.bloom_precheck_for_paths(
                    odb,
                    commit_oid,
                    bloom_keys,
                    bloom_cwd,
                    changed_paths_version,
                    read_changed_paths,
                )?;
                if let Some(stats) = bloom_stats {
                    if let Ok(mut g) = stats.lock() {
                        g.record_precheck(bloom_ret);
                    }
                }
                if bloom_ret == BloomPrecheck::DefinitelyNot {
                    return Ok(false);
                }
            }
        }
        if pathspecs.is_empty() {
            return Ok(true);
        }
        let entries = diff_trees(odb, None, Some(&info.tree), "")?;
        let touches = entries.iter().any(|e| {
            let path = e.path();
            pathspecs.iter().any(|ps| path_matches(path, ps))
        });
        if bloom_ret == BloomPrecheck::Maybe && !touches {
            if let Some(stats) = bloom_stats {
                if let Ok(mut g) = stats.lock() {
                    g.record_false_positive();
                }
            }
        }
        return Ok(touches);
    }

    if info.parents.len() == 1 {
        let mut bloom_ret = BloomPrecheck::Inapplicable;
        if let Some(chain) = bloom_chain {
            if !bloom_keys.is_empty() {
                bloom_ret = chain.bloom_precheck_for_paths(
                    odb,
                    commit_oid,
                    bloom_keys,
                    bloom_cwd,
                    changed_paths_version,
                    read_changed_paths,
                )?;
                if let Some(stats) = bloom_stats {
                    if let Ok(mut g) = stats.lock() {
                        g.record_precheck(bloom_ret);
                    }
                }
                if bloom_ret == BloomPrecheck::DefinitelyNot {
                    return Ok(false);
                }
            }
        }
        if pathspecs.is_empty() {
            return Ok(true);
        }
        let parent_obj = odb.read(&info.parents[0])?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        let entries = diff_trees(odb, Some(&parent_commit.tree), Some(&info.tree), "")?;
        let touches = entries.iter().any(|e| {
            let path = e.path();
            pathspecs.iter().any(|ps| path_matches(path, ps))
        });
        if bloom_ret == BloomPrecheck::Maybe && !touches {
            if let Some(stats) = bloom_stats {
                if let Ok(mut g) = stats.lock() {
                    g.record_false_positive();
                }
            }
        }
        return Ok(touches);
    }

    for parent_oid in &info.parents {
        let parent_obj = odb.read(parent_oid)?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        let entries = diff_trees(odb, Some(&parent_commit.tree), Some(&info.tree), "")?;
        if entries.iter().any(|e| {
            let path = e.path();
            pathspecs.iter().any(|ps| path_matches(path, ps))
        }) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if a file path matches a pathspec (prefix match or exact match).
fn path_matches(path: &str, pathspec: &str) -> bool {
    crate::pathspec::pathspec_matches(pathspec, path)
}

/// Extract unix timestamp from an author/committer line.
/// Read the committer timestamp from a commit object for priority queue ordering.
fn read_commit_timestamp(odb: &Odb, oid: &ObjectId) -> i64 {
    match odb.read(oid) {
        Ok(obj) => match parse_commit(&obj.data) {
            Ok(commit) => committer_unix_seconds_for_ordering(&commit.committer),
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

fn read_author_timestamp(odb: &Odb, oid: &ObjectId) -> i64 {
    match odb.read(oid) {
        Ok(obj) => match parse_commit(&obj.data) {
            Ok(commit) => committer_unix_seconds_for_ordering(&commit.author),
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

fn extract_timestamp(ident: &str) -> String {
    match timestamp_for_at_ct(signature_timestamp_for_pretty(ident)) {
        Some(ts) => ts.to_string(),
        None => String::new(),
    }
}

fn format_relative_from_diff(diff: i64) -> String {
    if diff < 0 {
        "in the future".to_owned()
    } else if diff < 60 {
        format!("{} seconds ago", diff)
    } else if diff < 3600 {
        let m = diff / 60;
        if m == 1 {
            "1 minute ago".to_owned()
        } else {
            format!("{m} minutes ago")
        }
    } else if diff < 86400 {
        let h = diff / 3600;
        if h == 1 {
            "1 hour ago".to_owned()
        } else {
            format!("{h} hours ago")
        }
    } else if diff < 86400 * 30 {
        let d = diff / 86400;
        if d == 1 {
            "1 day ago".to_owned()
        } else {
            format!("{d} days ago")
        }
    } else if diff < 86400 * 365 {
        let months = diff / (86400 * 30);
        if months == 1 {
            "1 month ago".to_owned()
        } else {
            format!("{months} months ago")
        }
    } else {
        let years = diff / (86400 * 365);
        if years == 1 {
            "1 year ago".to_owned()
        } else {
            format!("{years} years ago")
        }
    }
}

/// Lazily loads the git-notes map on first use so `grit log` does not read every
/// note before printing the first commit (e.g. oneline output never touches notes).
struct NotesMapCache<'a> {
    repo: &'a Repository,
    map: Option<std::collections::HashMap<ObjectId, Vec<u8>>>,
}

impl<'a> NotesMapCache<'a> {
    fn new(repo: &'a Repository) -> Self {
        Self { repo, map: None }
    }

    fn repo(&self) -> &'a Repository {
        self.repo
    }

    fn map(&mut self) -> &std::collections::HashMap<ObjectId, Vec<u8>> {
        if self.map.is_none() {
            self.map = Some(load_notes_map(self.repo));
        }
        self.map.as_ref().unwrap()
    }
}

/// Load notes from the configured notes ref (or `refs/notes/commits` default).
/// Returns a map from commit OID to the notes blob OID.
fn load_notes_map(repo: &Repository) -> std::collections::HashMap<ObjectId, Vec<u8>> {
    use grit_lib::config::ConfigSet;

    let notes_ref = std::env::var("GIT_NOTES_REF")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
            config
                .get("core.notesRef")
                .unwrap_or_else(|| "refs/notes/commits".to_string())
        });
    load_notes_map_for_ref(repo, &notes_ref)
}

fn load_notes_map_for_ref(
    repo: &Repository,
    notes_ref: &str,
) -> std::collections::HashMap<ObjectId, Vec<u8>> {
    use grit_lib::refs::resolve_ref;

    let mut map = std::collections::HashMap::new();

    // Resolve notes ref to a commit, then get its tree
    let notes_oid = match resolve_ref(&repo.git_dir, notes_ref) {
        Ok(oid) => oid,
        Err(_) => return map,
    };

    let obj = match repo.odb.read(&notes_oid) {
        Ok(o) => o,
        Err(_) => return map,
    };

    let tree_oid = match obj.kind {
        ObjectKind::Commit => match parse_commit(&obj.data) {
            Ok(c) => c.tree,
            Err(_) => return map,
        },
        ObjectKind::Tree => notes_oid,
        _ => return map,
    };

    collect_notes_map_recursive(repo, &tree_oid, String::new(), &mut map);
    map
}

/// Concatenate two note blobs the same way as Git's `combine_notes_concatenate` in `notes.c`:
/// empty `new` leaves `cur` unchanged; empty or missing `cur` becomes `new`; otherwise join with
/// `\n\n` after stripping one trailing newline from `cur`.
fn combine_notes_concatenate_blobs(cur: &[u8], new: &[u8]) -> Vec<u8> {
    if new.is_empty() {
        return cur.to_vec();
    }
    if cur.is_empty() {
        return new.to_vec();
    }
    let mut cur_len = cur.len();
    if cur_len > 0 && cur[cur_len - 1] == b'\n' {
        cur_len -= 1;
    }
    let mut out = Vec::with_capacity(cur_len.saturating_add(2).saturating_add(new.len()));
    out.extend_from_slice(&cur[..cur_len]);
    out.push(b'\n');
    out.push(b'\n');
    out.extend_from_slice(new);
    out
}

fn collect_notes_map_recursive(
    repo: &Repository,
    tree_oid: &grit_lib::objects::ObjectId,
    prefix: String,
    map: &mut std::collections::HashMap<grit_lib::objects::ObjectId, Vec<u8>>,
) {
    use grit_lib::objects::parse_tree;
    let tree_obj = match repo.odb.read(tree_oid) {
        Ok(o) => o,
        Err(_) => return,
    };
    let entries = match parse_tree(&tree_obj.data) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        let full_hex = format!("{prefix}{name}");
        if entry.mode == 0o040000 {
            collect_notes_map_recursive(repo, &entry.oid, full_hex, map);
        } else if let Ok(commit_oid) = full_hex.parse::<grit_lib::objects::ObjectId>() {
            if let Ok(blob) = repo.odb.read(&entry.oid) {
                use std::collections::hash_map::Entry;
                match map.entry(commit_oid) {
                    Entry::Vacant(e) => {
                        e.insert(blob.data);
                    }
                    Entry::Occupied(mut e) => {
                        // Same as Git when two tree paths resolve to one commit id: skip if the note
                        // blob is identical (`combine_notes` short-circuit on matching oids).
                        if e.get().as_slice() == blob.data.as_slice() {
                            continue;
                        }
                        let combined =
                            combine_notes_concatenate_blobs(e.get(), blob.data.as_slice());
                        e.insert(combined);
                    }
                }
            }
        }
    }
}

/// Whether `write_notes` would emit anything for this commit (used for inter-commit spacing).
fn commit_has_notes_to_show(
    oid: &ObjectId,
    notes_cache: &mut NotesMapCache<'_>,
    args: &Args,
) -> bool {
    if args.no_notes {
        return false;
    }
    if args.notes_refs.is_empty() {
        return notes_cache.map().contains_key(oid);
    }
    for spec in &args.notes_refs {
        let refname = if spec.is_empty() {
            "refs/notes/commits".to_owned()
        } else {
            let s = spec.as_str();
            if s.starts_with("refs/") {
                s.to_owned()
            } else {
                format!("refs/notes/{s}")
            }
        };
        if load_notes_map_for_ref(notes_cache.repo(), &refname).contains_key(oid) {
            return true;
        }
    }
    false
}

/// Write notes for a commit if any exist.
fn write_notes(
    out: &mut impl Write,
    oid: &ObjectId,
    notes_cache: &mut NotesMapCache<'_>,
    args: &Args,
    _odb: &Odb,
) -> Result<()> {
    if args.no_notes {
        return Ok(());
    }
    if args.notes_refs.is_empty() {
        let notes_map = notes_cache.map();
        if let Some(note_data) = notes_map.get(oid) {
            let note_text = String::from_utf8_lossy(note_data);
            writeln!(out)?;
            writeln!(out, "Notes:")?;
            for line in note_text.lines() {
                writeln!(out, "    {line}")?;
            }
        }
        return Ok(());
    }
    for spec in &args.notes_refs {
        let (header, refname) = if spec.is_empty() {
            ("Notes".to_owned(), "refs/notes/commits".to_owned())
        } else {
            let s = spec.as_str();
            let refname = if s.starts_with("refs/") {
                s.to_owned()
            } else {
                format!("refs/notes/{s}")
            };
            let short = s.strip_prefix("refs/notes/").unwrap_or(s);
            (format!("Notes ({short})"), refname)
        };
        let map = load_notes_map_for_ref(notes_cache.repo(), &refname);
        if let Some(note_data) = map.get(oid) {
            let note_text = String::from_utf8_lossy(note_data);
            writeln!(out)?;
            writeln!(out, "{header}:")?;
            for line in note_text.lines() {
                writeln!(out, "    {line}")?;
            }
        }
    }
    Ok(())
}

fn validate_log_pickaxe_options(repo: &Repository, args: &Args) -> Result<()> {
    if args.no_pickaxe_regex {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: unrecognized argument: --no-pickaxe-regex".to_string(),
        }));
    }
    if let Some(s) = args.pickaxe_string.as_deref() {
        if s == "\u{7f}__GRIT_MISSING_PICKAXE_S__" {
            return Err(anyhow::Error::new(ExplicitExit {
                code: 129,
                message: "error: switch `S' requires a value".to_string(),
            }));
        }
        if s.is_empty() {
            return Err(anyhow::Error::new(ExplicitExit {
                code: 129,
                message: "error: -S requires a non-empty argument".to_string(),
            }));
        }
    }
    if let Some(s) = args.pickaxe_grep.as_deref() {
        if s == "\u{7f}__GRIT_MISSING_PICKAXE_G__" {
            return Err(anyhow::Error::new(ExplicitExit {
                code: 129,
                message: "error: switch `G' requires a value".to_string(),
            }));
        }
        if s.is_empty() {
            return Err(anyhow::Error::new(ExplicitExit {
                code: 129,
                message: "error: -G requires a non-empty argument".to_string(),
            }));
        }
    }
    if args.pickaxe_grep.is_some() && args.pickaxe_regex {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: options '-G' and '--pickaxe-regex' cannot be used together, use '--pickaxe-regex' with '-S'".to_string(),
        }));
    }

    let mut pickaxe_kinds = 0usize;
    if args.pickaxe_grep.is_some() {
        pickaxe_kinds += 1;
    }
    if args.pickaxe_string.is_some() {
        pickaxe_kinds += 1;
    }
    if args.find_object.is_some() {
        pickaxe_kinds += 1;
    }
    if args.pickaxe_all && args.find_object.is_some() {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: options '--pickaxe-all' and '--find-object' cannot be used together, use '--pickaxe-all' with '-G' and '-S'".to_string(),
        }));
    }
    if pickaxe_kinds > 1 {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 128,
            message: "fatal: options '-G', '-S', and '--find-object' cannot be used together"
                .to_string(),
        }));
    }

    if (args.pickaxe_grep.is_some() || args.pickaxe_string.is_some()) && !args.no_textconv {
        validate_pickaxe_textconv_drivers(repo.git_dir.as_path(), repo.work_tree.as_deref())?;
    }
    Ok(())
}

/// First executable token of `diff.<driver>.textconv`, matching Git's shell concatenation for
/// values like `"/abs/cwd"/hexdump` (t4030-diff-textconv).
fn pickaxe_textconv_cmd_first_token(cmd_line: &str) -> Option<String> {
    let s = cmd_line.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        let prefix = &rest[..end];
        let tail = rest[end + 1..].trim_start();
        let suffix = tail.split_whitespace().next().unwrap_or("");
        return Some(format!("{prefix}{suffix}"));
    }
    let first = s.split_whitespace().next()?;
    Some(first.trim_matches(|c| c == '"' || c == '\'').to_string())
}

fn path_has_textconv_driver(git_dir: &Path, config: &ConfigSet, path: &str) -> bool {
    let work_tree = git_dir.parent().unwrap_or(git_dir);
    let rules = load_gitattributes(work_tree);
    let fa = get_file_attrs(&rules, path, false, config);
    if let DiffAttr::Driver(ref driver) = fa.diff_attr {
        return config.get(&format!("diff.{driver}.textconv")).is_some();
    }
    false
}

fn validate_pickaxe_textconv_drivers(git_dir: &Path, work_tree: Option<&Path>) -> Result<()> {
    let Some(wt) = work_tree else {
        return Ok(());
    };
    let rules = load_gitattributes(wt);
    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    let mut drivers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rule in &rules {
        for d in rule.diff_drivers() {
            drivers.insert(d.to_owned());
        }
    }
    for driver in drivers {
        let Some(cmd_line) = config.get(&format!("diff.{driver}.textconv")) else {
            continue;
        };
        let mut cmd_line = cmd_line.trim_end().to_string();
        if cmd_line.ends_with('<') {
            cmd_line = cmd_line.trim_end_matches('<').trim_end().to_string();
        }
        let Some(first_word) = pickaxe_textconv_cmd_first_token(&cmd_line) else {
            continue;
        };
        if first_word.starts_with('/') || first_word.contains('/') {
            if !Path::new(&first_word).is_file() {
                return Err(anyhow::Error::new(ExplicitExit {
                    code: 128,
                    message: format!(
                        "error: cannot run {}: No such file or directory\nfatal: unable to read files to diff",
                        first_word
                    ),
                }));
            }
            continue;
        }
        let exists = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {first_word} >/dev/null 2>&1"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !exists {
            return Err(anyhow::Error::new(ExplicitExit {
                code: 128,
                message: format!(
                    "error: cannot run {first_word}: No such file or directory\nfatal: unable to read files to diff"
                ),
            }));
        }
    }
    Ok(())
}

fn commit_pickaxe_matches(
    git_dir: &Path,
    odb: &Odb,
    info: &CommitInfo,
    args: &Args,
) -> Result<bool> {
    let entries = compute_commit_diff(odb, info)?;
    let use_textconv = !args.no_textconv;
    let config = ConfigSet::load(Some(git_dir), true).unwrap_or_default();

    let grep_re = if let Some(ref pat) = args.pickaxe_grep {
        Some(
            RegexBuilder::new(pat)
                .case_insensitive(args.regexp_ignore_case)
                .build()
                .with_context(|| format!("invalid pickaxe regex: {pat}"))?,
        )
    } else {
        None
    };

    let s_pickaxe_re = if args.pickaxe_regex {
        let needle = args
            .pickaxe_string
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("internal: --pickaxe-regex without -S"))?;
        Some(
            RegexBuilder::new(needle)
                .case_insensitive(args.regexp_ignore_case)
                .build()
                .with_context(|| format!("invalid pickaxe regex: {needle}"))?,
        )
    } else {
        None
    };

    for entry in &entries {
        let path = entry.path();
        let old_raw = read_blob_bytes(odb, &entry.old_oid);
        let new_raw = read_blob_bytes(odb, &entry.new_oid);

        if grep_re.is_some() && !args.text {
            let has_textconv_driver =
                use_textconv && path_has_textconv_driver(git_dir, &config, path);
            let old_bin = is_binary_for_diff(git_dir, path, &old_raw);
            let new_bin = is_binary_for_diff(git_dir, path, &new_raw);
            // Match Git diffcore_pickaxe: skip -G unless `-a` or a textconv applies to a binary side.
            if (!has_textconv_driver && old_bin) || (!has_textconv_driver && new_bin) {
                continue;
            }
        }

        let old_text = blob_text_for_diff(git_dir, &config, path, &old_raw, use_textconv);
        let new_text = blob_text_for_diff(git_dir, &config, path, &new_raw, use_textconv);

        if let Some(ref re) = grep_re {
            let patch = unified_diff(
                old_text.as_str(),
                new_text.as_str(),
                entry.old_path.as_deref().unwrap_or(path),
                entry.new_path.as_deref().unwrap_or(path),
                3,
            );
            if pickaxe_g_matches_diff_lines(re, &patch) {
                return Ok(true);
            }
            continue;
        }

        if let Some(ref needle) = args.pickaxe_string {
            if args.pickaxe_regex {
                let re = s_pickaxe_re.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("internal: --pickaxe-regex without compiled regex")
                })?;
                let old_c = re.find_iter(old_text.as_str()).count();
                let new_c = re.find_iter(new_text.as_str()).count();
                if old_c != new_c {
                    return Ok(true);
                }
            } else if args.regexp_ignore_case && needle.is_ascii() {
                let old_c = count_ascii_case_insensitive(&old_text, needle);
                let new_c = count_ascii_case_insensitive(&new_text, needle);
                if old_c != new_c {
                    return Ok(true);
                }
            } else {
                let old_c = old_text.matches(needle.as_str()).count();
                let new_c = new_text.matches(needle.as_str()).count();
                if old_c != new_c {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn read_blob_bytes(odb: &Odb, oid: &ObjectId) -> Vec<u8> {
    if oid.is_zero() {
        return Vec::new();
    }
    odb.read(oid).map(|o| o.data).unwrap_or_default()
}

/// Match Git's `diffgrep_consume`: run the regex on each added/removed line's **body** (the byte
/// sequence after the single `+` / `-` hunk prefix), not on diff headers or `++` / `--` lines.
fn pickaxe_g_matches_diff_lines(re: &Regex, patch: &str) -> bool {
    for line in patch.lines() {
        let b = line.as_bytes();
        let body_start = match b.first().copied() {
            Some(b'+') if b.get(1).copied() != Some(b'+') => 1,
            Some(b'-') if b.get(1).copied() != Some(b'-') => 1,
            _ => continue,
        };
        let body = line.get(body_start..).unwrap_or("");
        if re.is_match(body) {
            return true;
        }
    }
    false
}

fn count_ascii_case_insensitive(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let hay = haystack.as_bytes();
    let nd = needle.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i + nd.len() <= hay.len() {
        let mut matched = true;
        for j in 0..nd.len() {
            if !hay[i + j].eq_ignore_ascii_case(&nd[j]) {
                matched = false;
                break;
            }
        }
        if matched {
            count += 1;
            i += nd.len();
        } else {
            i += 1;
        }
    }
    count
}

/// Post-walk filters applied after [`walk_commits`] (diff-filter, find-object, decoration, dates).
fn commit_passes_post_walk_filters(
    repo: &Repository,
    odb: &Odb,
    oid: &ObjectId,
    info: &CommitInfo,
    args: &Args,
    diff_filter: Option<&str>,
    find_oid: Option<ObjectId>,
    find_object_tree_recursive: bool,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
    since_threshold: Option<i64>,
    until_threshold: Option<i64>,
) -> Result<bool> {
    if let Some(filter) = diff_filter {
        let include_chars: Vec<char> = filter.chars().filter(|c| c.is_uppercase()).collect();
        let exclude_chars: Vec<char> = filter
            .chars()
            .filter(|c| c.is_lowercase())
            .map(|c| c.to_uppercase().next().unwrap_or(c))
            .collect();
        let passes = if args.remerge_diff && info.parents.len() == 2 {
            if !include_chars.is_empty() {
                commit_has_remerge_diff_status(repo, info, &include_chars).unwrap_or(true)
            } else if !exclude_chars.is_empty() {
                commit_has_remerge_diff_status_not_in(repo, info, &exclude_chars).unwrap_or(true)
            } else {
                true
            }
        } else if !include_chars.is_empty() {
            commit_has_diff_status(odb, info, &include_chars).unwrap_or(true)
        } else if !exclude_chars.is_empty() {
            commit_has_diff_status_not_in(odb, info, &exclude_chars).unwrap_or(true)
        } else {
            true
        };
        if !passes {
            return Ok(false);
        }
    }
    if let Some(fo) = find_oid {
        let has = if args.remerge_diff && info.parents.len() == 2 {
            commit_has_remerge_object(repo, info, &fo).unwrap_or_default()
        } else {
            commit_has_object(odb, info, &fo, find_object_tree_recursive).unwrap_or_default()
        };
        if !has {
            return Ok(false);
        }
    }
    if args.remerge_diff {
        if let Some(ref p) = args.pickaxe_string {
            if info.parents.len() != 2 {
                return Ok(false);
            }
            if !commit_remerge_pickaxe_matches(repo, info, p.as_bytes())? {
                return Ok(false);
            }
        }
    }
    if args.simplify_by_decoration {
        if let Some(dec_map) = decorations {
            if !dec_map.contains_key(&oid.to_hex()) {
                return Ok(false);
            }
        }
    }
    if let Some(t) = since_threshold {
        if extract_epoch_from_ident(&info.committer) < t {
            return Ok(false);
        }
    }
    if let Some(t) = until_threshold {
        if extract_epoch_from_ident(&info.committer) > t {
            return Ok(false);
        }
    }
    Ok(true)
}

fn run_symmetric_log(repo: &Repository, args: &Args, _patch_context: usize) -> Result<()> {
    let mut lhs: Option<String> = None;
    let mut rhs: Option<String> = None;
    for rev in &args.revisions {
        if rev == "--" {
            break;
        }
        if rev.starts_with('-') && !rev.starts_with('^') {
            continue;
        }
        if is_symmetric_diff(rev) {
            if let Some((l, r)) = split_symmetric_diff(rev) {
                lhs = Some(l);
                rhs = Some(r);
            }
        }
    }
    let (lhs, rhs) = match (lhs, rhs) {
        (Some(l), Some(r)) => (l, r),
        _ => anyhow::bail!("symmetric revision required"),
    };

    let lhs_oid = grit_lib::rev_parse::resolve_revision(repo, &lhs)
        .with_context(|| format!("bad revision '{lhs}'"))?;
    let rhs_oid = grit_lib::rev_parse::resolve_revision(repo, &rhs)
        .with_context(|| format!("bad revision '{rhs}'"))?;
    let bases = merge_bases(repo, lhs_oid, rhs_oid, args.first_parent)
        .context("failed to compute merge bases for symmetric range")?;
    let negative: Vec<String> = bases.iter().map(|b| b.to_hex()).collect();

    let positive = vec![lhs, rhs];
    let options = RevListOptions {
        left_right: true,
        left_only: args.left_only,
        right_only: args.right_only,
        symmetric_left: Some(lhs_oid),
        symmetric_right: Some(rhs_oid),
        boundary: args.boundary,
        first_parent: args.first_parent,
        ordering: OrderingMode::Topo,
        reverse: false,
        ..RevListOptions::default()
    };
    let result = rev_list(repo, &positive, &negative, &options).context("rev-list failed")?;

    let boundary_set: HashSet<ObjectId> = result.boundary_commits.iter().copied().collect();
    let mut ordered = result.commits.clone();
    if args.boundary {
        for b in &result.boundary_commits {
            ordered.push(*b);
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut notes_cache = NotesMapCache::new(repo);
    let mut prev_had_notes = false;

    for (i, oid) in ordered.iter().enumerate() {
        let this_has_notes = commit_has_notes_to_show(oid, &mut notes_cache, args);
        if i > 0 && prev_had_notes {
            writeln!(out)?;
        }
        let is_boundary = boundary_set.contains(oid);
        let log_marker = if is_boundary {
            Some('-')
        } else {
            match result.left_right_map.get(oid) {
                Some(true) => Some('<'),
                Some(false) => Some('>'),
                None => None,
            }
        };
        let info = load_commit_info(repo, *oid)?;
        format_commit(
            &mut out,
            oid,
            &info,
            args,
            None,
            false,
            &mut notes_cache,
            &repo.odb,
            None,
            false,
            log_marker,
            None,
            None,
        )?;
        prev_had_notes = this_has_notes;
    }

    Ok(())
}

/// Format and print a single commit.
///
/// When `parent_line_override` is set (e.g. `log --parents` after line-log rewrite), `%p` / `%P`
/// and the `Merge:` header use these hashes instead of the raw commit parents.
fn format_commit(
    out: &mut impl Write,
    oid: &ObjectId,
    info: &CommitInfo,
    args: &Args,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
    use_color: bool,
    notes_cache: &mut NotesMapCache<'_>,
    odb: &Odb,
    parent_line_override: Option<&[ObjectId]>,
    _line_log: bool,
    log_marker: Option<char>,
    merge_from_parent: Option<&ObjectId>,
    source_for_oneline: Option<&str>,
) -> Result<()> {
    let hex = oid.to_hex();
    let abbrev_len = if args.no_abbrev {
        40
    } else {
        parse_abbrev(&args.abbrev)
    };
    let display_parents = parent_line_override.unwrap_or(info.parents.as_slice());
    let merge_suffix = merge_from_parent
        .map(|p| {
            let h = p.to_hex();
            format!(" (from {})", &h[..abbrev_len.min(h.len())])
        })
        .unwrap_or_default();

    if args.oneline || args.format.as_deref() == Some("oneline") {
        let first_line = info.message.lines().next().unwrap_or("");
        let abbrev = &hex[..abbrev_len.min(hex.len())];
        if let Some(src) = source_for_oneline {
            writeln!(out, "{abbrev}\t{src} {first_line}")?;
        } else {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "{abbrev}{dec} {first_line}")?;
        }
        return Ok(());
    }

    let format = args.format.as_deref();
    let date_format = args.date.as_deref();

    match format {
        Some(fmt) if fmt.starts_with("format:") || fmt.starts_with("tformat:") => {
            let is_tformat = fmt.starts_with("tformat:");
            let template = if let Some(t) = fmt.strip_prefix("format:") {
                t
            } else {
                &fmt[8..]
            };
            let note_bytes = notes_cache.map().get(oid).map(Vec::as_slice);
            let formatted = apply_format_string(
                template,
                oid,
                info,
                decorations,
                date_format,
                abbrev_len,
                use_color,
                note_bytes,
                display_parents,
                log_marker,
            );
            if is_tformat {
                if args.null_terminator {
                    write!(out, "{formatted}\0")?;
                } else {
                    writeln!(out, "{formatted}")?;
                }
            } else {
                write!(out, "{formatted}")?;
            }
        }
        Some("short") => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{merge_suffix}{dec}")?;
            if display_parents.len() > 1 {
                let parent_abbrevs: Vec<String> = display_parents
                    .iter()
                    .map(|p| {
                        let h = p.to_hex();
                        h[..abbrev_len.min(h.len())].to_string()
                    })
                    .collect();
                writeln!(out, "Merge: {}", parent_abbrevs.join(" "))?;
            }
            let author_name = extract_name(&info.author);
            writeln!(out, "Author: {author_name}")?;
            writeln!(out)?;
            for line in info.message.lines().take(1) {
                writeln!(out, "    {line}")?;
            }
            writeln!(out)?;
        }
        Some("medium") | None => {
            let dec = format_decoration(&hex, decorations);
            if use_color {
                writeln!(out, "\x1b[33mcommit {hex}{merge_suffix}\x1b[m{dec}")?;
            } else {
                writeln!(out, "commit {hex}{merge_suffix}{dec}")?;
            }
            if display_parents.len() > 1 {
                let parent_abbrevs: Vec<String> = display_parents
                    .iter()
                    .map(|p| {
                        let h = p.to_hex();
                        h[..abbrev_len.min(h.len())].to_string()
                    })
                    .collect();
                writeln!(out, "Merge: {}", parent_abbrevs.join(" "))?;
            }
            writeln!(out, "Author: {}", format_ident_display(&info.author))?;
            writeln!(
                out,
                "Date:   {}",
                format_author_date_internal(&info.author, date_format, true)
            )?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            write_notes(out, oid, notes_cache, args, odb)?;
        }
        Some("full") => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{merge_suffix}{dec}")?;
            if display_parents.len() > 1 {
                let parent_abbrevs: Vec<String> = display_parents
                    .iter()
                    .map(|p| {
                        let h = p.to_hex();
                        h[..abbrev_len.min(h.len())].to_string()
                    })
                    .collect();
                writeln!(out, "Merge: {}", parent_abbrevs.join(" "))?;
            }
            writeln!(out, "Author: {}", format_ident_display(&info.author))?;
            writeln!(out, "Commit: {}", format_ident_display(&info.committer))?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            write_notes(out, oid, notes_cache, args, odb)?;
        }
        Some("fuller") => {
            let dec = format_decoration(&hex, decorations);
            writeln!(out, "commit {hex}{merge_suffix}{dec}")?;
            if display_parents.len() > 1 {
                let parent_abbrevs: Vec<String> = display_parents
                    .iter()
                    .map(|p| {
                        let h = p.to_hex();
                        h[..abbrev_len.min(h.len())].to_string()
                    })
                    .collect();
                writeln!(out, "Merge: {}", parent_abbrevs.join(" "))?;
            }
            writeln!(out, "Author:     {}", format_ident_display(&info.author))?;
            writeln!(
                out,
                "AuthorDate: {}",
                format_author_date_internal(&info.author, date_format, true)
            )?;
            writeln!(out, "Commit:     {}", format_ident_display(&info.committer))?;
            writeln!(
                out,
                "CommitDate: {}",
                format_author_date_internal(&info.committer, date_format, true)
            )?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            write_notes(out, oid, notes_cache, args, odb)?;
        }
        Some("reference") => {
            let subject = info.message.lines().next().unwrap_or("");
            let line = grit_lib::commit_pretty::format_reference_line(
                oid,
                subject,
                &info.committer,
                abbrev_len,
            );
            writeln!(out, "{line}")?;
        }
        Some(other) => {
            // Try as a format string directly
            let note_bytes = notes_cache.map().get(oid).map(Vec::as_slice);
            let formatted = apply_format_string(
                other,
                oid,
                info,
                decorations,
                date_format,
                abbrev_len,
                use_color,
                note_bytes,
                display_parents,
                log_marker,
            );
            writeln!(out, "{formatted}")?;
        }
    }

    Ok(())
}

/// Apply a format string with placeholders like %H, %h, %s, %an, %ae, etc.
fn apply_format_string(
    template: &str,
    oid: &ObjectId,
    info: &CommitInfo,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
    date_format: Option<&str>,
    abbrev_len: usize,
    use_color: bool,
    notes_raw: Option<&[u8]>,
    display_parents: &[ObjectId],
    log_marker: Option<char>,
) -> String {
    let hex = oid.to_hex();

    // Alignment/truncation helpers
    #[derive(Clone, Copy)]
    enum Align {
        Left,
        Right,
        Center,
    }
    #[derive(Clone, Copy)]
    enum Trunc {
        None,
        Trunc,
        LTrunc,
        MTrunc,
    }
    struct ColSpec {
        width: usize,
        align: Align,
        trunc: Trunc,
        absolute: bool,
    }
    fn apply_col(spec: &ColSpec, s: &str) -> String {
        let char_len = s.chars().count();
        if char_len > spec.width {
            match spec.trunc {
                Trunc::None => s.to_owned(),
                Trunc::Trunc => {
                    let mut out: String = s.chars().take(spec.width.saturating_sub(2)).collect();
                    out.push_str("..");
                    out
                }
                Trunc::LTrunc => {
                    let skip = char_len - spec.width + 2;
                    let mut out = String::from("..");
                    out.extend(s.chars().skip(skip));
                    out
                }
                Trunc::MTrunc => {
                    let keep = spec.width.saturating_sub(2);
                    let left = keep / 2;
                    let right = keep - left;
                    let mut out: String = s.chars().take(left).collect();
                    out.push_str("..");
                    out.extend(s.chars().skip(char_len - right));
                    out
                }
            }
        } else {
            let pad = spec.width - char_len;
            match spec.align {
                Align::Left => {
                    let mut o = s.to_owned();
                    for _ in 0..pad {
                        o.push(' ');
                    }
                    o
                }
                Align::Right => {
                    let mut o = String::new();
                    for _ in 0..pad {
                        o.push(' ');
                    }
                    o.push_str(s);
                    o
                }
                Align::Center => {
                    let l = pad / 2;
                    let r = pad - l;
                    let mut o = String::new();
                    for _ in 0..l {
                        o.push(' ');
                    }
                    o.push_str(s);
                    for _ in 0..r {
                        o.push(' ');
                    }
                    o
                }
            }
        }
    }
    fn parse_col_spec(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        align: Align,
    ) -> Option<ColSpec> {
        // Check for | (absolute column) variant
        let absolute = if chars.peek() == Some(&'|') {
            chars.next();
            true
        } else {
            false
        };
        if chars.peek() != Some(&'(') {
            return None;
        }
        chars.next();
        // Parse number (may be negative)
        let negative = if chars.peek() == Some(&'-') {
            chars.next();
            true
        } else {
            false
        };
        let mut num_str = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                num_str.push(c);
                chars.next();
            } else {
                break;
            }
        }
        let mut width: usize = num_str.parse().ok()?;
        if negative {
            // Negative means COLUMNS - N; default terminal width is 80
            let columns = std::env::var("COLUMNS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(80);
            width = columns.saturating_sub(width);
        }
        let trunc = if chars.peek() == Some(&',') {
            chars.next();
            let mut mode = String::new();
            while let Some(&c) = chars.peek() {
                if c == ')' {
                    break;
                }
                mode.push(c);
                chars.next();
            }
            match mode.as_str() {
                "trunc" => Trunc::Trunc,
                "ltrunc" => Trunc::LTrunc,
                "mtrunc" => Trunc::MTrunc,
                _ => Trunc::None,
            }
        } else {
            Trunc::None
        };
        if chars.peek() == Some(&')') {
            chars.next();
        }
        Some(ColSpec {
            width,
            align,
            trunc,
            absolute,
        })
    }

    let mut pending_col: Option<ColSpec> = None;
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            // Check alignment directives
            if chars.peek() == Some(&'<') {
                chars.next();
                if let Some(spec) = parse_col_spec(&mut chars, Align::Left) {
                    pending_col = Some(spec);
                }
                continue;
            }
            if chars.peek() == Some(&'>') {
                chars.next();
                if chars.peek() == Some(&'<') {
                    chars.next();
                    if let Some(spec) = parse_col_spec(&mut chars, Align::Center) {
                        pending_col = Some(spec);
                    }
                } else if chars.peek() == Some(&'>') {
                    chars.next();
                    if let Some(spec) = parse_col_spec(&mut chars, Align::Right) {
                        pending_col = Some(spec);
                    }
                } else if let Some(spec) = parse_col_spec(&mut chars, Align::Right) {
                    pending_col = Some(spec);
                }
                continue;
            }

            let col_start = if pending_col.is_some() {
                result.len()
            } else {
                0
            };
            match chars.peek() {
                Some('H') => {
                    chars.next();
                    result.push_str(&hex);
                }
                Some('h') => {
                    chars.next();
                    result.push_str(&hex[..abbrev_len.min(hex.len())]);
                }
                Some('T') => {
                    chars.next();
                    result.push_str(&info.tree.to_hex());
                }
                Some('t') => {
                    chars.next();
                    let th = info.tree.to_hex();
                    result.push_str(&th[..abbrev_len.min(th.len())]);
                }
                Some('P') => {
                    chars.next();
                    let parents: Vec<String> = display_parents.iter().map(|p| p.to_hex()).collect();
                    result.push_str(&parents.join(" "));
                }
                Some('p') => {
                    chars.next();
                    let parents: Vec<String> = display_parents
                        .iter()
                        .map(|p| {
                            let ph = p.to_hex();
                            ph[..abbrev_len.min(ph.len())].to_owned()
                        })
                        .collect();
                    result.push_str(&parents.join(" "));
                }
                Some('a') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(&info.author));
                        }
                        Some('N') => {
                            chars.next();
                            result.push_str(&extract_name(&info.author));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(&info.author));
                        }
                        Some('E') => {
                            chars.next();
                            result.push_str(&extract_email(&info.author));
                        }
                        Some('l') => {
                            chars.next();
                            result.push_str(&extract_email_local(&info.author));
                        }
                        Some('d') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.author, date_format));
                        }
                        Some('D') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.author, Some("rfc")));
                        }
                        Some('t') => {
                            chars.next();
                            result.push_str(&extract_timestamp(&info.author).to_string());
                        }
                        Some('s') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.author, Some("short")));
                        }
                        Some('i') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.author, Some("iso")));
                        }
                        Some('I') => {
                            chars.next();
                            result
                                .push_str(&format_date_with_mode(&info.author, Some("iso-strict")));
                        }
                        Some('r') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.author, Some("relative")));
                        }
                        _ => result.push_str("%a"),
                    }
                }
                Some('c') => {
                    chars.next();
                    match chars.peek() {
                        Some('n') => {
                            chars.next();
                            result.push_str(&extract_name(&info.committer));
                        }
                        Some('N') => {
                            chars.next();
                            result.push_str(&extract_name(&info.committer));
                        }
                        Some('e') => {
                            chars.next();
                            result.push_str(&extract_email(&info.committer));
                        }
                        Some('E') => {
                            chars.next();
                            result.push_str(&extract_email(&info.committer));
                        }
                        Some('l') => {
                            chars.next();
                            result.push_str(&extract_email_local(&info.committer));
                        }
                        Some('d') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.committer, date_format));
                        }
                        Some('D') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.committer, Some("rfc")));
                        }
                        Some('t') => {
                            chars.next();
                            result.push_str(&extract_timestamp(&info.committer).to_string());
                        }
                        Some('s') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.committer, Some("short")));
                        }
                        Some('i') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(&info.committer, Some("iso")));
                        }
                        Some('I') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(
                                &info.committer,
                                Some("iso-strict"),
                            ));
                        }
                        Some('r') => {
                            chars.next();
                            result.push_str(&format_date_with_mode(
                                &info.committer,
                                Some("relative"),
                            ));
                        }
                        _ => result.push_str("%c"),
                    }
                }
                Some('s') => {
                    chars.next();
                    result.push_str(info.message.lines().next().unwrap_or(""));
                }
                Some('b') => {
                    chars.next();
                    // Body: everything after the first paragraph separator (blank line)
                    let body = extract_body(&info.message);
                    result.push_str(&body);
                }
                Some('B') => {
                    chars.next();
                    if !info.message.is_empty() {
                        result.push_str(&info.message);
                        if !info.message.ends_with('\n') {
                            result.push('\n');
                        }
                    }
                }
                Some('d') => {
                    chars.next();
                    // Decorations
                    let dec = format_decoration(&hex, decorations);
                    result.push_str(&dec);
                }
                Some('D') => {
                    chars.next();
                    // Decorations without parens
                    let dec = format_decoration_no_parens(&hex, decorations);
                    result.push_str(&dec);
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('m') => {
                    chars.next();
                    if let Some(c) = log_marker {
                        result.push(c);
                    }
                }
                Some('N') => {
                    chars.next();
                    if let Some(raw) = notes_raw {
                        result.push_str(&String::from_utf8_lossy(raw));
                    }
                }
                Some('%') => {
                    chars.next();
                    result.push('%');
                }
                Some('C') => {
                    chars.next();
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        let mut spec = String::new();
                        for c in chars.by_ref() {
                            if c == ')' {
                                break;
                            }
                            spec.push(c);
                        }
                        let (force, color_spec) = if let Some(rest) = spec.strip_prefix("always,") {
                            (true, rest)
                        } else if let Some(rest) = spec.strip_prefix("auto,") {
                            (false, rest)
                        } else if spec == "auto" {
                            if use_color {
                                result.push_str("\x1b[m");
                            }
                            continue;
                        } else {
                            (false, spec.as_str())
                        };
                        if use_color || force {
                            result.push_str(&format_ansi_color_spec(color_spec));
                        }
                    } else {
                        let remaining: String = chars.clone().collect();
                        let known = [
                            "reset", "red", "green", "blue", "yellow", "magenta", "cyan", "white",
                            "bold", "dim", "ul",
                        ];
                        let mut matched = false;
                        for name in &known {
                            if remaining.starts_with(name) {
                                for _ in 0..name.len() {
                                    chars.next();
                                }
                                if use_color {
                                    result.push_str(&format_ansi_color_name(name));
                                }
                                matched = true;
                                break;
                            }
                        }
                        if !matched {
                            while let Some(&c) = chars.peek() {
                                if c.is_alphanumeric() {
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
                Some('x') => {
                    // Hex escape: %xNN
                    chars.next();
                    let mut hex_str = String::new();
                    if let Some(&c1) = chars.peek() {
                        if c1.is_ascii_hexdigit() {
                            hex_str.push(c1);
                            chars.next();
                        }
                    }
                    if let Some(&c2) = chars.peek() {
                        if c2.is_ascii_hexdigit() {
                            hex_str.push(c2);
                            chars.next();
                        }
                    }
                    if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                        result.push(byte as char);
                    }
                }
                Some('w') => {
                    // %w(...) wrapping directive — consume and ignore
                    chars.next();
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        for c in chars.by_ref() {
                            if c == ')' {
                                break;
                            }
                        }
                    }
                }
                Some('e') => {
                    // Encoding
                    chars.next();
                }
                Some('g') => {
                    // Reflog placeholders (%gD, %gd, %gs, etc.) — empty for non-reflog
                    chars.next();
                    if let Some(&_nc) = chars.peek() {
                        chars.next();
                    }
                }
                _ => result.push('%'),
            }
            // Apply pending column formatting
            if let Some(spec) = pending_col.take() {
                let added = result[col_start..].to_owned();
                result.truncate(col_start);
                if spec.absolute {
                    // Absolute column: pad from start of current line to target column
                    let line_start = result.rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let current_col = result[line_start..].chars().count();
                    let target_width = spec.width.saturating_sub(current_col);
                    let mut adjusted_spec = ColSpec {
                        width: target_width,
                        align: spec.align,
                        trunc: spec.trunc,
                        absolute: false,
                    };
                    // For absolute positioning, ensure minimum width matches the value length
                    if target_width < added.chars().count() {
                        adjusted_spec.width = added.chars().count();
                    }
                    result.push_str(&apply_col(&adjusted_spec, &added));
                } else {
                    result.push_str(&apply_col(&spec, &added));
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extract the message body (everything after the subject + blank line).
fn extract_body(message: &str) -> String {
    let msg = message.trim_end_matches('\n');
    let mut lines = msg.lines();
    // Skip subject line
    lines.next();
    // Skip blank line separator if present
    if let Some(line) = lines.next() {
        if !line.is_empty() {
            // No blank separator — include this line as body
            let rest: Vec<&str> = lines.collect();
            if rest.is_empty() {
                return format!("{line}\n");
            } else {
                return format!("{}\n{}\n", line, rest.join("\n"));
            }
        }
    }
    // Collect remaining lines as body
    let body_lines: Vec<&str> = lines.collect();
    if body_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", body_lines.join("\n"))
    }
}

/// Extract the name portion from a Git ident string.
fn extract_name(ident: &str) -> String {
    if let Some(bracket) = ident.find('<') {
        ident[..bracket].trim().to_owned()
    } else {
        ident.to_owned()
    }
}

/// Extract the email portion from a Git ident string.
fn extract_email(ident: &str) -> String {
    if let Some(start) = ident.find('<') {
        if let Some(end) = ident.find('>') {
            return ident[start + 1..end].to_owned();
        }
    }
    String::new()
}

fn format_ansi_color_name(name: &str) -> String {
    match name {
        "red" => "\x1b[31m".to_owned(),
        "green" => "\x1b[32m".to_owned(),
        "yellow" => "\x1b[33m".to_owned(),
        "blue" => "\x1b[34m".to_owned(),
        "magenta" => "\x1b[35m".to_owned(),
        "cyan" => "\x1b[36m".to_owned(),
        "white" => "\x1b[37m".to_owned(),
        "bold" => "\x1b[1m".to_owned(),
        "dim" => "\x1b[2m".to_owned(),
        "ul" | "underline" => "\x1b[4m".to_owned(),
        "reset" => "\x1b[m".to_owned(),
        _ => String::new(),
    }
}

fn format_ansi_color_spec(spec: &str) -> String {
    if spec == "reset" {
        return "\x1b[m".to_owned();
    }
    fn color_code(name: &str) -> Option<u8> {
        match name {
            "black" => Some(0),
            "red" => Some(1),
            "green" => Some(2),
            "yellow" => Some(3),
            "blue" => Some(4),
            "magenta" => Some(5),
            "cyan" => Some(6),
            "white" => Some(7),
            "default" => Some(9),
            _ => None,
        }
    }
    let mut codes = Vec::new();
    let mut fg_set = false;
    for part in spec.split_whitespace() {
        match part {
            "bold" => codes.push("1".to_owned()),
            "dim" => codes.push("2".to_owned()),
            "italic" => codes.push("3".to_owned()),
            "ul" | "underline" => codes.push("4".to_owned()),
            "blink" => codes.push("5".to_owned()),
            "reverse" => codes.push("7".to_owned()),
            "strike" => codes.push("9".to_owned()),
            "nobold" | "nodim" => codes.push("22".to_owned()),
            "noitalic" => codes.push("23".to_owned()),
            "noul" | "nounderline" => codes.push("24".to_owned()),
            "noblink" => codes.push("25".to_owned()),
            "noreverse" => codes.push("27".to_owned()),
            "nostrike" => codes.push("29".to_owned()),
            _ => {
                if let Some(c) = color_code(part) {
                    if !fg_set {
                        codes.push(format!("{}", 30 + c));
                        fg_set = true;
                    } else {
                        codes.push(format!("{}", 40 + c));
                    }
                }
            }
        }
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

/// Extract the local part (before @) of the email from a Git ident string.
fn extract_email_local(ident: &str) -> String {
    let email = extract_email(ident);
    if let Some(at) = email.find('@') {
        email[..at].to_owned()
    } else {
        email
    }
}

/// Format ident for display: "Name <email>".
fn format_ident_display(ident: &str) -> String {
    let name = extract_name(ident);
    let email = extract_email(ident);
    format!("{name} <{email}>")
}

/// Format the date from an ident string for display, with optional date mode.
///
/// When `for_header` is true (pretty `Date:` lines), unparsable dates use the Unix epoch in UTC
/// (`+0000`), matching Git. When false (`%ad` and other format placeholders), unparsable dates
/// yield an empty string for the default format (t4212).
fn format_author_date_internal(ident: &str, date_mode: Option<&str>, for_header: bool) -> String {
    let tail = parse_signature_tail(ident);
    let (ts, tz_offset_secs, offset_str) = match tail {
        Some(SignatureTail::Valid(p)) => {
            let off = ident
                .get(p.tz_hhmm_range.clone())
                .unwrap_or("+0000")
                .to_owned();
            (p.unix_seconds, p.tz_offset_secs, off)
        }
        Some(SignatureTail::Overflow) if for_header => (0i64, 0i64, "+0000".to_owned()),
        Some(SignatureTail::Overflow) => {
            return match date_mode {
                None => "Thu Jan 1 00:00:00 1970 +0000".to_owned(),
                Some("relative") => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    format_relative_from_diff(now)
                }
                _ => String::new(),
            };
        }
        Some(SignatureTail::NonNumeric) if for_header => (0i64, 0i64, "+0000".to_owned()),
        Some(SignatureTail::NonNumeric) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            return match date_mode {
                Some("relative") => format_relative_from_diff(now),
                _ => String::new(),
            };
        }
        None if for_header => (0i64, 0i64, "+0000".to_owned()),
        None => return String::new(),
    };

    match date_mode {
        Some("short") => {
            // YYYY-MM-DD in the author's timezone
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            format!("{:04}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day())
        }
        Some("iso") | Some("iso8601") => {
            // ISO format: 2005-04-07 15:13:13 +0200
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}",
                dt.year(),
                dt.month() as u8,
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                offset_str
            )
        }
        Some("iso-strict") | Some("iso8601-strict") => {
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let sign = if tz_offset_secs >= 0 { '+' } else { '-' };
            let abs_offset = tz_offset_secs.unsigned_abs();
            let h = abs_offset / 3600;
            let m = (abs_offset % 3600) / 60;
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                dt.year(),
                dt.month() as u8,
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                sign,
                h,
                m
            )
        }
        Some("raw") => {
            format!("{ts} {offset_str}")
        }
        Some("relative") => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            format_relative_from_diff(now - ts)
        }
        Some("rfc") | Some("rfc2822") => {
            // RFC 2822: Thu, 07 Apr 2005 22:13:13 +0200
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let weekday = match dt.weekday() {
                time::Weekday::Monday => "Mon",
                time::Weekday::Tuesday => "Tue",
                time::Weekday::Wednesday => "Wed",
                time::Weekday::Thursday => "Thu",
                time::Weekday::Friday => "Fri",
                time::Weekday::Saturday => "Sat",
                time::Weekday::Sunday => "Sun",
            };
            let month = match dt.month() {
                time::Month::January => "Jan",
                time::Month::February => "Feb",
                time::Month::March => "Mar",
                time::Month::April => "Apr",
                time::Month::May => "May",
                time::Month::June => "Jun",
                time::Month::July => "Jul",
                time::Month::August => "Aug",
                time::Month::September => "Sep",
                time::Month::October => "Oct",
                time::Month::November => "Nov",
                time::Month::December => "Dec",
            };
            format!(
                "{}, {} {} {} {:02}:{:02}:{:02} {}",
                weekday,
                dt.day(),
                month,
                dt.year(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                offset_str
            )
        }
        Some("unix") => {
            format!("{ts}")
        }
        _ => {
            // Default Git date format: "Thu Apr 7 15:13:13 2005 -0700" (single space before day;
            // matches C git `show_date`).
            let adjusted = ts + tz_offset_secs;
            let dt = time::OffsetDateTime::from_unix_timestamp(adjusted)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let weekday = match dt.weekday() {
                time::Weekday::Monday => "Mon",
                time::Weekday::Tuesday => "Tue",
                time::Weekday::Wednesday => "Wed",
                time::Weekday::Thursday => "Thu",
                time::Weekday::Friday => "Fri",
                time::Weekday::Saturday => "Sat",
                time::Weekday::Sunday => "Sun",
            };
            let month = match dt.month() {
                time::Month::January => "Jan",
                time::Month::February => "Feb",
                time::Month::March => "Mar",
                time::Month::April => "Apr",
                time::Month::May => "May",
                time::Month::June => "Jun",
                time::Month::July => "Jul",
                time::Month::August => "Aug",
                time::Month::September => "Sep",
                time::Month::October => "Oct",
                time::Month::November => "Nov",
                time::Month::December => "Dec",
            };
            format!(
                "{} {} {} {:02}:{:02}:{:02} {} {}",
                weekday,
                month,
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                dt.year(),
                offset_str
            )
        }
    }
}

fn format_date_with_mode(ident: &str, date_mode: Option<&str>) -> String {
    format_author_date_internal(ident, date_mode, false)
}

/// Resolve a revision string to an ObjectId.
fn resolve_revision(repo: &Repository, rev: &str) -> Result<ObjectId> {
    // Delegate to the library's full revision parser which handles
    // @{N}, @{now}, @{upstream}, peeling, parent navigation, etc.
    grit_lib::rev_parse::resolve_revision(repo, rev)
        .map_err(|e| anyhow::anyhow!("unknown revision '{}': {}", rev, e))
}

fn resolve_revision_as_commit_after_precompose(repo: &Repository, rev: &str) -> Result<ObjectId> {
    if !grit_lib::precompose_config::effective_core_precomposeunicode(Some(&repo.git_dir))
        || !grit_lib::unicode_normalization::has_non_ascii_utf8(rev)
    {
        return resolve_revision_as_commit(repo, rev).map_err(|e| e.into());
    }
    let nfc = grit_lib::unicode_normalization::precompose_utf8_path(rev);
    if nfc.as_ref() == rev {
        return resolve_revision_as_commit(repo, rev).map_err(|e| e.into());
    }
    resolve_revision_as_commit(repo, nfc.as_ref()).map_err(|e| e.into())
}

/// Heuristic used for rev/pathspec DWIM when no `--` separator is present.
fn is_likely_pathspec_during_rev_parse(token: &str) -> bool {
    if token.contains("^{") || token.contains("@{") {
        return false;
    }

    // `..` in revision syntax is a range (e.g. `A..B`); single-token `..` is the parent dir pathspec.
    if token == ".." {
        return true;
    }
    if token.contains("..") {
        return false;
    }

    if token == "." {
        return true;
    }

    if let Some(rest) = token.strip_prefix(":/") {
        return rest.contains('*') || rest.contains('?') || rest.contains('[');
    }

    token.starts_with(":(")
        || token.contains('*')
        || token.contains('?')
        || token.contains('[')
        || token.contains(']')
}

/// Collect ref name → OID decorations from the repository.
fn collect_decorations(
    repo: &Repository,
    full: bool,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    // HEAD
    let head = resolve_head(&repo.git_dir)?;
    if let Some(oid) = head.oid() {
        let hex = oid.to_hex();
        let label = match &head {
            HeadState::Branch { short_name, .. } => {
                if full {
                    format!("HEAD -> refs/heads/{short_name}")
                } else {
                    format!("HEAD -> {short_name}")
                }
            }
            _ => "HEAD".to_owned(),
        };
        map.entry(hex).or_default().push(label);
    }

    // Use `list_refs` so packed refs (after `git pack-refs` / `tag -d`) are visible — scanning
    // `refs/heads` and `refs/tags` directories alone misses `packed-refs` (`t6016`).
    for (refname, oid) in grit_lib::refs::list_refs(&repo.git_dir, "refs/heads/")? {
        let hex = oid.to_hex();
        let short = refname
            .strip_prefix("refs/heads/")
            .unwrap_or(refname.as_str());
        let label = if full {
            format!("refs/heads/{short}")
        } else {
            short.to_owned()
        };
        map.entry(hex).or_default().push(label);
    }
    for (refname, oid) in grit_lib::refs::list_refs(&repo.git_dir, "refs/tags/")? {
        let resolved_hex =
            peel_to_commit_hex(&repo.odb, &oid.to_hex()).unwrap_or_else(|| oid.to_hex());
        let short = refname
            .strip_prefix("refs/tags/")
            .unwrap_or(refname.as_str());
        let label = if full {
            format!("tag: refs/tags/{short}")
        } else {
            format!("tag: {short}")
        };
        map.entry(resolved_hex).or_default().push(label);
    }

    // De-duplicate while preserving order.
    for refs in map.values_mut() {
        let mut seen = std::collections::HashSet::new();
        refs.retain(|r| seen.insert(r.clone()));
    }

    Ok(map)
}

/// Peel an object (possibly a tag) down to a commit and return its hex.
fn peel_to_commit_hex(odb: &Odb, hex: &str) -> Option<String> {
    use grit_lib::objects::ObjectKind;
    let oid: ObjectId = hex.parse().ok()?;
    let obj = odb.read(&oid).ok()?;
    match obj.kind {
        ObjectKind::Commit => Some(hex.to_owned()),
        ObjectKind::Tag => {
            let text = std::str::from_utf8(&obj.data).ok()?;
            for line in text.lines() {
                if let Some(target) = line.strip_prefix("object ") {
                    let target_hex = target.trim();
                    return peel_to_commit_hex(odb, target_hex);
                }
            }
            None
        }
        _ => None,
    }
}

/// Format decoration string for a commit (with parentheses).
fn format_decoration(
    hex: &str,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> String {
    match decorations {
        Some(map) => {
            if let Some(refs) = map.get(hex) {
                format!(" ({})", refs.join(", "))
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}

/// Format decoration string without parentheses (for %D).
fn format_decoration_no_parens(
    hex: &str,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> String {
    match decorations {
        Some(map) => {
            if let Some(refs) = map.get(hex) {
                refs.join(", ")
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}

// ── Diff output for log ──────────────────────────────────────────────

/// Compute combined diff entries: only files that differ from ALL parents.
fn compute_combined_diff_entries(odb: &Odb, info: &CommitInfo) -> Result<Vec<DiffEntry>> {
    use std::collections::HashSet;
    // For each parent, find files that are different from that parent
    let mut changed_per_parent: Vec<HashSet<String>> = Vec::new();
    for parent_oid in &info.parents {
        let parent_obj = odb.read(parent_oid)?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        let entries = diff_trees(odb, Some(&parent_commit.tree), Some(&info.tree), "")?;
        let paths: HashSet<String> = entries.iter().map(|e| e.path().to_string()).collect();
        changed_per_parent.push(paths);
    }
    // Intersection: only files changed from ALL parents
    if changed_per_parent.is_empty() {
        return Ok(vec![]);
    }
    let mut common = changed_per_parent[0].clone();
    for other in &changed_per_parent[1..] {
        common = common.intersection(other).cloned().collect();
    }
    if common.is_empty() {
        return Ok(vec![]);
    }
    // Get entries from first-parent diff that are in common set
    let first_parent_obj = odb.read(&info.parents[0])?;
    let first_parent_commit = parse_commit(&first_parent_obj.data)?;
    let entries = diff_trees(odb, Some(&first_parent_commit.tree), Some(&info.tree), "")?;
    Ok(entries
        .into_iter()
        .filter(|e| common.contains(e.path()))
        .collect())
}

/// Compute diff entries for a commit against its first parent (or empty tree for root commits).
fn compute_commit_diff(odb: &Odb, info: &CommitInfo) -> Result<Vec<DiffEntry>> {
    if info.parents.is_empty() {
        // Root commit: diff against empty tree
        Ok(diff_trees(odb, None, Some(&info.tree), "")?)
    } else {
        let parent_obj = odb.read(&info.parents[0])?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        Ok(diff_trees(
            odb,
            Some(&parent_commit.tree),
            Some(&info.tree),
            "",
        )?)
    }
}

fn compute_commit_diff_against_parent(
    odb: &Odb,
    info: &CommitInfo,
    parent_idx: usize,
) -> Result<Vec<DiffEntry>> {
    if parent_idx >= info.parents.len() {
        return Ok(Vec::new());
    }
    let parent_obj = odb.read(&info.parents[parent_idx])?;
    let parent_commit = parse_commit(&parent_obj.data)?;
    Ok(diff_trees(
        odb,
        Some(&parent_commit.tree),
        Some(&info.tree),
        "",
    )?)
}

/// Write diff output for a single commit.
fn write_commit_diff(
    out: &mut impl Write,
    repo: &Repository,
    commit_oid: &ObjectId,
    info: &CommitInfo,
    args: &Args,
    pathspecs: &[String],
    graph_stat_prefix: Option<&str>,
    decorations: Option<&std::collections::HashMap<String, Vec<String>>>,
    use_color: bool,
    notes_cache: &mut NotesMapCache<'_>,
    patch_context: usize,
) -> Result<()> {
    let odb = &repo.odb;
    let git_dir = &repo.git_dir;
    let is_merge = info.parents.len() > 1;

    if !log_commit_needs_diff_output(args, info, git_dir)? {
        return Ok(());
    }

    if merge_diff_is_remerge(args, is_merge, git_dir)? && info.parents.len() == 2 {
        use crate::commands::remerge_diff::{write_remerge_diff, RemergeDiffOptions};
        let find_oid = if let Some(ref s) = args.find_object {
            Some(grit_lib::rev_parse::resolve_revision(repo, s)?)
        } else {
            None
        };
        let opts = RemergeDiffOptions {
            pathspecs,
            diff_filter: args.diff_filter.as_deref(),
            // Pickaxe filters which commits appear; the displayed remerge diff is always full.
            pickaxe: None,
            find_object: find_oid,
            submodule_mode: None,
            context_lines: patch_context,
        };
        return write_remerge_diff(out, repo, &info.tree, &info.parents, &opts);
    }

    let show_patch = log_wants_patch_hunks(args, info, git_dir)?;
    let separate = merge_diff_is_separate(args, is_merge, git_dir)?;

    let log_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if separate {
        for (i, parent_oid) in info.parents.iter().enumerate() {
            let mut entries = compute_commit_diff_against_parent(odb, info, i)?;
            if entries.is_empty() {
                continue;
            }
            if let Some(ref order_path) = args.order_file {
                entries =
                    crate::commands::diff::apply_orderfile_entries(entries, order_path, &log_cwd)?;
            }
            entries = crate::commands::diff::apply_rotate_skip_log_entries(
                odb,
                &info.tree,
                entries,
                args.rotate_to.as_deref(),
                args.skip_to.as_deref(),
            )?;
            // First parent: the main `format_commit` was already printed; only extra headers
            // repeat the commit with `(from <parent>)` for parents 2+ (matches Git).
            if i > 0 {
                format_commit(
                    out,
                    commit_oid,
                    info,
                    args,
                    decorations,
                    use_color,
                    notes_cache,
                    odb,
                    None,
                    false,
                    None,
                    Some(parent_oid),
                    None,
                )?;
            }
            write_commit_diff_body(
                out,
                odb,
                git_dir,
                &entries,
                &entries,
                args,
                graph_stat_prefix,
                show_patch,
                false,
                patch_context,
            )?;
        }
        return Ok(());
    }

    let mut entries = compute_commit_diff(odb, info)?;
    if entries.is_empty() {
        return Ok(());
    }

    if let Some(ref order_path) = args.order_file {
        entries = crate::commands::diff::apply_orderfile_entries(entries, order_path, &log_cwd)?;
    }

    let combined_style = merge_diff_is_combined_style(args, is_merge, git_dir)?;
    let mut combined_entries = if combined_style {
        compute_combined_diff_entries(odb, info)?
    } else {
        entries.clone()
    };

    entries = crate::commands::diff::apply_rotate_skip_log_entries(
        odb,
        &info.tree,
        entries,
        args.rotate_to.as_deref(),
        args.skip_to.as_deref(),
    )?;
    combined_entries = if combined_style {
        crate::commands::diff::apply_rotate_skip_log_entries(
            odb,
            &info.tree,
            combined_entries,
            args.rotate_to.as_deref(),
            args.skip_to.as_deref(),
        )?
    } else {
        entries.clone()
    };

    write_commit_diff_body(
        out,
        odb,
        git_dir,
        &entries,
        &combined_entries,
        args,
        graph_stat_prefix,
        show_patch,
        is_merge,
        patch_context,
    )?;

    Ok(())
}

fn write_commit_diff_body(
    out: &mut impl Write,
    odb: &Odb,
    git_dir: &Path,
    entries: &[DiffEntry],
    combined_entries: &[DiffEntry],
    args: &Args,
    graph_stat_prefix: Option<&str>,
    show_patch: bool,
    treat_as_merge_for_format: bool,
    patch_context: usize,
) -> Result<()> {
    let combined_style = merge_diff_is_combined_style(args, treat_as_merge_for_format, git_dir)?;
    let list_raw_name = if combined_style {
        combined_entries
    } else {
        entries
    };
    let list_patch = if combined_style {
        combined_entries
    } else {
        entries
    };
    let has_patch = show_patch && !list_patch.is_empty();

    if args.raw {
        for entry in list_raw_name {
            writeln!(out, "{}", format_raw(entry))?;
        }
        writeln!(out)?;
    }

    if !args.stat.is_empty() {
        if has_patch {
            writeln!(out, "---")?;
        } else {
            writeln!(out)?;
        }
        log_print_stat_summary(
            out,
            odb,
            entries,
            has_patch,
            args,
            graph_stat_prefix,
            git_dir,
        )?;
    }

    if args.name_only {
        for entry in list_raw_name {
            writeln!(out, "{}", entry.path())?;
        }
        writeln!(out)?;
    }

    if args.name_status {
        for entry in list_raw_name {
            writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
        }
        writeln!(out)?;
    }

    if show_patch {
        let config = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
        for entry in list_patch {
            log_write_patch_entry(out, odb, git_dir, &config, entry, args, patch_context)?;
        }
    }

    Ok(())
}

/// Write a unified-diff block for one entry.
fn log_write_patch_entry(
    out: &mut impl Write,
    odb: &Odb,
    git_dir: &std::path::Path,
    config: &ConfigSet,
    entry: &DiffEntry,
    args: &Args,
    context_lines: usize,
) -> Result<()> {
    let old_path = entry
        .old_path
        .as_deref()
        .unwrap_or(entry.new_path.as_deref().unwrap_or(""));
    let new_path = entry
        .new_path
        .as_deref()
        .unwrap_or(entry.old_path.as_deref().unwrap_or(""));

    if args.no_prefix {
        writeln!(out, "diff --git {old_path} {new_path}")?;
    } else {
        writeln!(out, "diff --git a/{old_path} b/{new_path}")?;
    }

    match entry.status {
        DiffStatus::Added => {
            writeln!(out, "new file mode {}", entry.new_mode)?;
            writeln!(
                out,
                "index {}..{}",
                &entry.old_oid.to_hex()[..7],
                &entry.new_oid.to_hex()[..7]
            )?;
        }
        DiffStatus::Deleted => {
            writeln!(out, "deleted file mode {}", entry.old_mode)?;
            writeln!(
                out,
                "index {}..{}",
                &entry.old_oid.to_hex()[..7],
                &entry.new_oid.to_hex()[..7]
            )?;
        }
        DiffStatus::Modified => {
            if entry.old_mode != entry.new_mode {
                writeln!(out, "old mode {}", entry.old_mode)?;
                writeln!(out, "new mode {}", entry.new_mode)?;
            }
            if entry.old_mode == entry.new_mode {
                writeln!(
                    out,
                    "index {}..{} {}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7],
                    entry.old_mode
                )?;
            } else {
                writeln!(
                    out,
                    "index {}..{}",
                    &entry.old_oid.to_hex()[..7],
                    &entry.new_oid.to_hex()[..7]
                )?;
            }
        }
        DiffStatus::Renamed => {
            writeln!(out, "similarity index 100%")?;
            writeln!(out, "rename from {old_path}")?;
            writeln!(out, "rename to {new_path}")?;
        }
        DiffStatus::Copied => {
            writeln!(out, "similarity index 100%")?;
            writeln!(out, "copy from {old_path}")?;
            writeln!(out, "copy to {new_path}")?;
        }
        DiffStatus::TypeChanged => {
            writeln!(out, "old mode {}", entry.old_mode)?;
            writeln!(out, "new mode {}", entry.new_mode)?;
        }
        DiffStatus::Unmerged => {}
    }

    let path_for_attrs = entry.path();
    let use_textconv = !args.no_textconv;
    let textconv_patch = use_textconv && diff_textconv_active(git_dir, config, path_for_attrs);
    let old_raw = read_blob_bytes(odb, &entry.old_oid);
    let new_raw = read_blob_bytes(odb, &entry.new_oid);
    if !textconv_patch
        && (is_binary_for_diff(git_dir, path_for_attrs, &old_raw)
            || is_binary_for_diff(git_dir, path_for_attrs, &new_raw))
    {
        let (src_pfx, dst_pfx) = if args.no_prefix {
            ("", "")
        } else {
            ("a/", "b/")
        };
        writeln!(
            out,
            "Binary files {src_pfx}{old_path} and {dst_pfx}{new_path} differ"
        )?;
        return Ok(());
    }

    let old_content = if entry.old_oid == zero_oid() {
        String::new()
    } else if use_textconv {
        blob_text_for_diff_with_oid(
            odb,
            git_dir,
            config,
            path_for_attrs,
            &old_raw,
            &entry.old_oid,
            true,
        )
    } else {
        String::from_utf8_lossy(&old_raw).into_owned()
    };
    let new_content = if entry.new_oid == zero_oid() {
        String::new()
    } else if use_textconv {
        blob_text_for_diff_with_oid(
            odb,
            git_dir,
            config,
            path_for_attrs,
            &new_raw,
            &entry.new_oid,
            true,
        )
    } else {
        String::from_utf8_lossy(&new_raw).into_owned()
    };
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
    let (src_pfx, dst_pfx) = if args.no_prefix {
        ("", "")
    } else {
        ("a/", "b/")
    };
    let patch = grit_lib::diff::unified_diff_with_prefix(
        &old_content,
        &new_content,
        display_old,
        display_new,
        context_lines,
        0,
        src_pfx,
        dst_pfx,
    );
    let patch = apply_diff_output_indicators(&patch, args);
    write!(out, "{patch}")?;

    Ok(())
}

fn apply_diff_output_indicators(patch: &str, args: &Args) -> String {
    if args.output_indicator_new.is_none()
        && args.output_indicator_old.is_none()
        && args.output_indicator_context.is_none()
    {
        return patch.to_owned();
    }
    let new_c = args
        .output_indicator_new
        .as_deref()
        .and_then(|s| s.chars().next())
        .unwrap_or('>');
    let old_c = args
        .output_indicator_old
        .as_deref()
        .and_then(|s| s.chars().next())
        .unwrap_or('<');
    let ctx_c = args
        .output_indicator_context
        .as_deref()
        .and_then(|s| s.chars().next())
        .unwrap_or('#');
    let mut out = String::with_capacity(patch.len());
    for line in patch.split_inclusive('\n') {
        let bytes = line.as_bytes();
        if bytes.first() == Some(&b'+') && bytes.get(1) != Some(&b'+') && !line.starts_with("+++ ")
        {
            out.push(new_c);
            out.push_str(&line[1..]);
        } else if bytes.first() == Some(&b'-')
            && bytes.get(1) != Some(&b'-')
            && !line.starts_with("--- ")
        {
            out.push(old_c);
            out.push_str(&line[1..]);
        } else if bytes.first() == Some(&b' ') {
            out.push(ctx_c);
            out.push_str(&line[1..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Write a `--stat` summary for log.
fn log_print_stat_summary(
    out: &mut impl Write,
    odb: &Odb,
    entries: &[DiffEntry],
    trailing_blank: bool,
    args: &Args,
    graph_line_prefix: Option<&str>,
    git_dir: &Path,
) -> Result<()> {
    let use_color = log_resolve_stdout_color(args, git_dir);
    let cfg = ConfigSet::load(Some(git_dir), true).unwrap_or_default();
    let eff_name_width = args.stat_name_width.or_else(|| {
        cfg.get("diff.statNameWidth")
            .and_then(|s| s.parse::<usize>().ok())
    });
    let cfg_stat_graph = cfg
        .get("diff.statGraphWidth")
        .and_then(|s| s.parse::<usize>().ok());
    let eff_graph_width = args.stat_graph_width.or(cfg_stat_graph);
    let graph_bar_slack = if graph_line_prefix.is_some() {
        if args.stat_graph_width.is_some() || cfg_stat_graph.is_some() || args.stat_width.is_some()
        {
            0
        } else {
            1
        }
    } else {
        0
    };
    let (color_add, color_del, color_reset) = if use_color {
        let add = cfg
            .get("color.diff.new")
            .and_then(|s| grit_lib::config::parse_color(&s).ok())
            .unwrap_or_else(|| "\x1b[32m".to_string());
        let del = cfg
            .get("color.diff.old")
            .and_then(|s| grit_lib::config::parse_color(&s).ok())
            .unwrap_or_else(|| "\x1b[31m".to_string());
        (add, del, "\x1b[m".to_string())
    } else {
        (String::new(), String::new(), String::new())
    };

    let line_prefix = graph_line_prefix.unwrap_or("");
    let subtract_prefix = graph_line_prefix.is_some() && args.stat_width.is_none();

    let mut files: Vec<FileStatInput> = Vec::with_capacity(entries.len());
    for entry in entries {
        let path_display = match entry.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                grit_lib::diff::format_rename_path(old, new)
            }
            _ => entry.path().to_string(),
        };
        let old_raw = if entry.old_oid == zero_oid() {
            Vec::new()
        } else {
            odb.read(&entry.old_oid).map(|o| o.data).unwrap_or_default()
        };
        let new_raw = if entry.new_oid == zero_oid() {
            Vec::new()
        } else {
            odb.read(&entry.new_oid).map(|o| o.data).unwrap_or_default()
        };
        let binary = old_raw.contains(&0) || new_raw.contains(&0);
        if binary {
            let deleted = if entry.old_oid == zero_oid() {
                0
            } else {
                old_raw.len()
            };
            let added = if entry.new_oid == zero_oid() {
                0
            } else {
                new_raw.len()
            };
            files.push(FileStatInput {
                path_display,
                insertions: added,
                deletions: deleted,
                is_binary: true,
            });
        } else {
            let (old_content, new_content) = log_read_blob_pair(odb, entry)?;
            let (ins, del) = count_changes(&old_content, &new_content);
            files.push(FileStatInput {
                path_display,
                insertions: ins,
                deletions: del,
                is_binary: false,
            });
        }
    }

    let opts = DiffstatOptions {
        total_width: args.stat_width.unwrap_or_else(terminal_columns),
        line_prefix,
        subtract_prefix_from_terminal: subtract_prefix,
        stat_name_width: eff_name_width,
        stat_graph_width: eff_graph_width,
        stat_count: args.stat_count,
        color_add: color_add.as_str(),
        color_del: color_del.as_str(),
        color_reset: color_reset.as_str(),
        graph_bar_slack,
        graph_prefix_budget_slack: if graph_line_prefix.is_some() && use_color {
            1
        } else {
            0
        },
    };
    write_diffstat_block(out, &files, &opts)?;
    if trailing_blank {
        writeln!(out)?;
    }

    Ok(())
}

/// Read both blob sides of a diff entry as UTF-8 strings.
fn log_read_blob_pair(odb: &Odb, entry: &DiffEntry) -> Result<(String, String)> {
    let zero = grit_lib::diff::zero_oid();

    let old_content = if entry.old_oid == zero {
        String::new()
    } else {
        match odb.read(&entry.old_oid) {
            Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
            Err(_) => String::new(),
        }
    };

    let new_content = if entry.new_oid == zero {
        String::new()
    } else {
        match odb.read(&entry.new_oid) {
            Ok(obj) => String::from_utf8_lossy(&obj.data).into_owned(),
            Err(_) => String::new(),
        }
    };

    Ok((old_content, new_content))
}

/// Collect all commit OIDs from all refs (branches, tags, etc.) for --all.
fn collect_all_ref_oids(git_dir: &std::path::Path) -> Result<Vec<ObjectId>> {
    use std::fs;
    let mut oids = Vec::new();
    let mut seen = HashSet::new();

    // Reftable backend
    if grit_lib::reftable::is_reftable_repo(git_dir) {
        if let Ok(refs) = grit_lib::reftable::reftable_list_refs(git_dir, "refs/") {
            for (_name, oid) in refs {
                if seen.insert(oid) {
                    oids.push(oid);
                }
            }
        }
        return Ok(oids);
    }

    // Loose refs
    collect_oids_from_dir(git_dir, &git_dir.join("refs"), &mut oids, &mut seen)?;

    // Packed refs
    let packed_path = git_dir.join("packed-refs");
    if let Ok(text) = fs::read_to_string(packed_path) {
        for line in text.lines() {
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            if let Some(hex) = line.split_whitespace().next() {
                if let Ok(oid) = hex.parse::<ObjectId>() {
                    if seen.insert(oid) {
                        oids.push(oid);
                    }
                }
            }
        }
    }

    Ok(oids)
}

fn collect_oids_from_dir(
    git_dir: &std::path::Path,
    dir: &std::path::Path,
    oids: &mut Vec<ObjectId>,
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    use std::fs;
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_oids_from_dir(git_dir, &entry.path(), oids, seen)?;
        } else if ft.is_file() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                let raw = content.trim();
                if let Some(target) = raw.strip_prefix("ref: ") {
                    // Symbolic ref — resolve it
                    if let Ok(oid) = grit_lib::refs::resolve_ref(git_dir, target) {
                        if seen.insert(oid) {
                            oids.push(oid);
                        }
                    }
                } else if let Ok(oid) = raw.parse::<ObjectId>() {
                    if seen.insert(oid) {
                        oids.push(oid);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Check if a commit does NOT have any changes of the excluded types (for lowercase diff-filter).
/// Returns true if NONE of the changes match the excluded types.
fn commit_has_diff_status_not_in(
    odb: &Odb,
    info: &CommitInfo,
    exclude_chars: &[char],
) -> Result<bool> {
    let parent_tree = if let Some(parent) = info.parents.first() {
        let pobj = odb.read(parent)?;
        let pc = parse_commit(&pobj.data)?;
        Some(pc.tree)
    } else {
        None
    };
    let entries = diff_trees(odb, parent_tree.as_ref(), Some(&info.tree), "")?;
    // Include commit if it has no changes of the excluded type
    Ok(!entries
        .iter()
        .any(|e| exclude_chars.contains(&e.status.letter())))
}

/// Check if a commit has any changes matching the specified diff-filter status letters.
fn commit_has_diff_status(odb: &Odb, info: &CommitInfo, filter_chars: &[char]) -> Result<bool> {
    let parent_tree = if let Some(parent) = info.parents.first() {
        let pobj = odb.read(parent)?;
        let pc = parse_commit(&pobj.data)?;
        Some(pc.tree)
    } else {
        None
    };

    let entries = diff_trees(odb, parent_tree.as_ref(), Some(&info.tree), "")?;
    for entry in &entries {
        let letter = entry.status.letter();
        if filter_chars.contains(&letter) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn blob_contains_pickaxe(odb: &Odb, oid: &ObjectId, needle: &[u8]) -> Result<bool> {
    if oid.is_zero() {
        return Ok(false);
    }
    let obj = odb.read(oid)?;
    Ok(obj.data.windows(needle.len()).any(|w| w == needle))
}

fn commit_remerge_pickaxe_matches(
    repo: &Repository,
    info: &CommitInfo,
    needle: &[u8],
) -> Result<bool> {
    for e in remerge_diff_entries(repo, info)? {
        if blob_contains_pickaxe(&repo.odb, &e.old_oid, needle)?
            || blob_contains_pickaxe(&repo.odb, &e.new_oid, needle)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remerge_diff_entries(repo: &Repository, info: &CommitInfo) -> Result<Vec<DiffEntry>> {
    use crate::commands::merge::remerge_merge_tree;
    use grit_lib::diff::detect_renames;

    if info.parents.len() != 2 {
        return Ok(Vec::new());
    }
    let (remerge_tree, _) = remerge_merge_tree(repo, info.parents[0], info.parents[1])?;
    let raw = diff_trees(&repo.odb, Some(&remerge_tree), Some(&info.tree), "")?;
    Ok(detect_renames(&repo.odb, raw, 50))
}

fn commit_has_remerge_diff_status(
    repo: &Repository,
    info: &CommitInfo,
    filter_chars: &[char],
) -> Result<bool> {
    for e in remerge_diff_entries(repo, info)? {
        if filter_chars.contains(&e.status.letter()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn commit_has_remerge_diff_status_not_in(
    repo: &Repository,
    info: &CommitInfo,
    exclude_chars: &[char],
) -> Result<bool> {
    Ok(!remerge_diff_entries(repo, info)?
        .iter()
        .any(|e| exclude_chars.contains(&e.status.letter())))
}

fn commit_has_remerge_object(
    repo: &Repository,
    info: &CommitInfo,
    target: &ObjectId,
) -> Result<bool> {
    for e in remerge_diff_entries(repo, info)? {
        if e.old_oid == *target || e.new_oid == *target {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check whether a commit's diff introduces or removes a specific object.
///
/// When `tree_in_recursive` is true, tree directory lines are included in the diff (Git
/// `tree_in_recursive` / `log -t`), which is required for `--find-object` on tree OIDs.
fn commit_has_object(
    odb: &Odb,
    info: &CommitInfo,
    target: &ObjectId,
    tree_in_recursive: bool,
) -> Result<bool> {
    if info.parents.len() > 1 {
        let walk = CombinedTreeDiffOptions {
            recursive: true,
            tree_in_recursive,
        };
        let paths =
            combined_diff_paths_filtered(odb, &info.tree, &info.parents, &walk, Some(target))?;
        return Ok(!paths.is_empty());
    }

    let parent_tree = if let Some(parent) = info.parents.first() {
        let pobj = odb.read(parent)?;
        let pc = parse_commit(&pobj.data)?;
        Some(pc.tree)
    } else {
        None
    };

    let entries = if tree_in_recursive {
        diff_trees_show_tree_entries(odb, parent_tree.as_ref(), Some(&info.tree), "")?
    } else {
        diff_trees(odb, parent_tree.as_ref(), Some(&info.tree), "")?
    };
    for entry in &entries {
        if entry.old_oid == *target || entry.new_oid == *target {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Filter commits by following a file across renames.
/// Returns only commits that touch the tracked file, updating the path
/// when renames are detected.
fn follow_filter(
    odb: &Odb,
    commits: Vec<(ObjectId, CommitInfo)>,
    initial_path: &str,
    max_count: Option<usize>,
) -> Result<Vec<(ObjectId, CommitInfo)>> {
    use grit_lib::diff::detect_renames;

    let mut tracked_path = initial_path.to_string();
    let mut result = Vec::new();

    for (oid, info) in commits {
        let parent_tree = if let Some(parent) = info.parents.first() {
            let pobj = odb.read(parent)?;
            let pc = parse_commit(&pobj.data)?;
            Some(pc.tree)
        } else {
            None
        };

        let raw_entries = diff_trees(odb, parent_tree.as_ref(), Some(&info.tree), "")?;
        let entries = detect_renames(odb, raw_entries, 50);

        let mut touches = false;
        for entry in &entries {
            match entry.status {
                DiffStatus::Renamed => {
                    // Check if the new path matches our tracked path
                    if let Some(new_path) = entry.new_path.as_deref() {
                        if new_path == tracked_path {
                            touches = true;
                            // Update tracked path to the old name for older commits
                            if let Some(old_path) = entry.old_path.as_deref() {
                                tracked_path = old_path.to_string();
                            }
                        }
                    }
                    // Also check old path
                    if let Some(old_path) = entry.old_path.as_deref() {
                        if old_path == tracked_path {
                            touches = true;
                        }
                    }
                }
                _ => {
                    let path = entry.path();
                    if path == tracked_path {
                        touches = true;
                    }
                }
            }
        }

        if touches {
            result.push((oid, info));
            if let Some(max) = max_count {
                if result.len() >= max {
                    break;
                }
            }
        }
    }

    Ok(result)
}

/// Build a map from commit OID → source ref name for --source.
/// For each ref, walk its commit ancestry and record the first ref that reaches each commit.
fn build_source_map(
    odb: &Odb,
    git_dir: &std::path::Path,
    first_parent: bool,
) -> Result<std::collections::HashMap<ObjectId, String>> {
    let mut source_map: std::collections::HashMap<ObjectId, String> =
        std::collections::HashMap::new();

    // Collect refs with names
    let refs = collect_all_refs_with_names(git_dir)?;

    for (oid, ref_name) in &refs {
        let mut queue = vec![*oid];
        let mut visited = HashSet::new();
        while let Some(commit_oid) = queue.pop() {
            if !visited.insert(commit_oid) {
                continue;
            }
            source_map
                .entry(commit_oid)
                .or_insert_with(|| ref_name.clone());
            if let Ok(obj) = odb.read(&commit_oid) {
                if let Ok(commit) = parse_commit(&obj.data) {
                    if first_parent {
                        if let Some(p) = commit.parents.first() {
                            queue.push(*p);
                        }
                    } else {
                        for p in &commit.parents {
                            if !visited.contains(p) {
                                queue.push(*p);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(source_map)
}

fn short_ref_for_source_display(src: &str) -> &str {
    if src == ".alternate" {
        return src;
    }
    src.strip_prefix("refs/heads/")
        .or_else(|| src.strip_prefix("refs/tags/"))
        .or_else(|| src.strip_prefix("refs/remotes/"))
        .unwrap_or(src)
}

/// Like [`build_source_map`] but walks alternate ref tips only, labeling every
/// reached commit with `.alternate` (Git `rev-list --alternate-refs` /
/// `log --source --alternate-refs`).
fn build_remote_tracking_source_map(
    odb: &Odb,
    git_dir: &std::path::Path,
    glob_pat: &str,
    first_parent: bool,
) -> Result<std::collections::HashMap<ObjectId, String>> {
    let mut source_map: std::collections::HashMap<ObjectId, String> =
        std::collections::HashMap::new();
    let refs = refs::list_refs_glob(git_dir, glob_pat)?;
    for (ref_name, oid) in refs {
        let mut queue = vec![oid];
        let mut visited = HashSet::new();
        while let Some(commit_oid) = queue.pop() {
            if !visited.insert(commit_oid) {
                continue;
            }
            source_map
                .entry(commit_oid)
                .or_insert_with(|| ref_name.clone());
            if let Ok(obj) = odb.read(&commit_oid) {
                if let Ok(commit) = parse_commit(&obj.data) {
                    if first_parent {
                        if let Some(p) = commit.parents.first() {
                            if !visited.contains(p) {
                                queue.push(*p);
                            }
                        }
                    } else {
                        for p in &commit.parents {
                            if !visited.contains(p) {
                                queue.push(*p);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(source_map)
}

fn build_alternate_source_map(
    repo: &Repository,
) -> Result<std::collections::HashMap<ObjectId, String>> {
    let mut source_map: std::collections::HashMap<ObjectId, String> =
        std::collections::HashMap::new();
    let tips = refs::collect_alternate_ref_oids(&repo.git_dir)?;
    let label = ".alternate".to_string();
    for tip in tips {
        let mut queue = vec![tip];
        let mut visited = HashSet::new();
        while let Some(commit_oid) = queue.pop() {
            if !visited.insert(commit_oid) {
                continue;
            }
            source_map
                .entry(commit_oid)
                .or_insert_with(|| label.clone());
            if let Ok(obj) = repo.odb.read(&commit_oid) {
                if let Ok(commit) = parse_commit(&obj.data) {
                    for p in &commit.parents {
                        if !visited.contains(p) {
                            queue.push(*p);
                        }
                    }
                }
            }
        }
    }
    Ok(source_map)
}

/// Collect all refs with their names from the repository.
fn collect_all_refs_with_names(git_dir: &std::path::Path) -> Result<Vec<(ObjectId, String)>> {
    let mut refs = Vec::new();

    // HEAD
    let head = resolve_head(git_dir)?;
    if let Some(oid) = head.oid() {
        refs.push((*oid, "HEAD".to_string()));
    }

    // Loose refs
    collect_named_refs_from_dir(git_dir, &git_dir.join("refs"), &mut refs)?;

    // Packed refs
    let packed_path = git_dir.join("packed-refs");
    if let Ok(text) = std::fs::read_to_string(packed_path) {
        for line in text.lines() {
            if line.starts_with('#') || line.starts_with('^') || line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() == 2 {
                if let Ok(oid) = parts[0].parse::<ObjectId>() {
                    refs.push((oid, parts[1].to_string()));
                }
            }
        }
    }

    Ok(refs)
}

/// Recursively collect refs with their full names from a directory.
fn collect_named_refs_from_dir(
    git_dir: &std::path::Path,
    dir: &std::path::Path,
    refs: &mut Vec<(ObjectId, String)>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_named_refs_from_dir(git_dir, &path, refs)?;
        } else if let Ok(content) = std::fs::read_to_string(&path) {
            let raw = content.trim();
            if let Some(target) = raw.strip_prefix("ref: ") {
                // Symbolic ref — resolve
                if let Ok(oid) = grit_lib::refs::resolve_ref(git_dir, target) {
                    let full_path = path.to_string_lossy();
                    if let Some(idx) = full_path.find("refs/") {
                        refs.push((oid, full_path[idx..].to_string()));
                    }
                }
            } else if let Ok(oid) = raw.parse::<ObjectId>() {
                let full_path = path.to_string_lossy();
                if let Some(idx) = full_path.find("refs/") {
                    refs.push((oid, full_path[idx..].to_string()));
                }
            }
        }
    }
    Ok(())
}

/// Parse the --abbrev value into a hash abbreviation length.
fn parse_abbrev(abbrev: &Option<String>) -> usize {
    match abbrev {
        Some(val) => val.parse::<usize>().unwrap_or(7),
        None => 7,
    }
}

/// Load shallow boundary commit OIDs from `.git/shallow`.
fn load_shallow_boundaries(git_dir: &Path) -> HashSet<ObjectId> {
    let shallow_path = git_dir.join("shallow");
    let mut set = HashSet::new();
    if let Ok(contents) = std::fs::read_to_string(&shallow_path) {
        for line in contents.lines() {
            let line = line.trim();
            if !line.is_empty() {
                if let Ok(oid) = line.parse::<ObjectId>() {
                    set.insert(oid);
                }
            }
        }
    }
    set
}

/// Resolve a pretty format alias by looking up `pretty.<name>` in git config.
/// Returns the resolved format string, or the input unchanged.
fn resolve_pretty_alias_with_config(fmt: &str, repo: &Repository) -> String {
    // Known built-in formats — no resolution needed
    match fmt {
        "oneline" | "short" | "medium" | "full" | "fuller" | "reference" | "email" | "raw"
        | "mboxrd" => {
            return fmt.to_string();
        }
        _ => {}
    }

    // Already a format: or tformat: string
    if fmt.starts_with("format:") || fmt.starts_with("tformat:") {
        return fmt.to_string();
    }

    // Try to resolve from config, with loop detection
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    let mut visited = std::collections::HashSet::new();
    let mut current = fmt.to_string();

    loop {
        if visited.contains(&current) {
            return current;
        }
        visited.insert(current.clone());

        let key = format!("pretty.{}", current);
        if let Some(value) = config.get(&key) {
            match value.as_str() {
                "oneline" | "short" | "medium" | "full" | "fuller" | "reference" | "email"
                | "raw" | "mboxrd" => {
                    return value;
                }
                v if v.starts_with("format:") || v.starts_with("tformat:") => {
                    return value;
                }
                _ => {
                    current = value;
                }
            }
        } else {
            return current;
        }
    }
}
