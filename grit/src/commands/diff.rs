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

use crate::explicit_exit::ExplicitExit;
use crate::pathspec::resolve_pathspec;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::attributes::{collect_attrs_for_path, load_gitattributes_for_diff, AttrValue};
use grit_lib::config::ConfigSet;
use grit_lib::crlf::{
    get_file_attrs, load_gitattributes, parse_gitattributes_content, AttrRule, DiffAttr,
};
use grit_lib::diff::{
    anchored_unified_diff, count_changes, count_changes_with_algorithm, count_git_lines,
    detect_renames, diff_index_to_tree, diff_index_to_worktree, diff_tree_to_worktree, diff_trees,
    diffcore_count_changes, empty_blob_oid, rewrite_dissimilarity_index_percent,
    should_break_rewrite_for_stat, unified_diff,
    unified_diff_with_prefix_and_funcname_and_algorithm, zero_oid, DiffEntry, DiffStatus,
};
use grit_lib::diffstat::{terminal_columns, write_diffstat_block, DiffstatOptions, FileStatInput};
use grit_lib::error::Error;
use grit_lib::index::Index;
use grit_lib::merge_diff::{
    blob_text_for_diff_with_oid, combined_diff_paths, diff_textconv_active,
    format_worktree_conflict_combined, run_textconv,
};
use grit_lib::objects::{parse_commit, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{rev_list, RevListOptions};
use grit_lib::rev_parse::{
    abbreviate_object_id, resolve_revision, resolve_treeish_blob_at_path, show_prefix,
    split_treeish_colon, TreeishBlobAtPath,
};
use grit_lib::userdiff::matcher_for_path_parsed;
use regex::Regex;
use similar::{ChangeTag, TextDiff};
use std::fmt::Write as FmtWrite;
use std::sync::Arc;

use crate::commands::diff_index::{write_patch_entry, SubmoduleIgnoreFlags};

/// Shared gitattributes + config for per-path diff algorithm selection (`diff.<driver>.algorithm`).
#[derive(Clone)]
struct DiffAlgoContext {
    attrs: Arc<grit_lib::attributes::ParsedGitAttributes>,
    config: Arc<grit_lib::config::ConfigSet>,
    ignore_case_attrs: bool,
}

/// Map a Git `diff.algorithm` / driver algorithm string to a `similar` line algorithm.
fn match_grit_diff_algorithm_name(name: &str) -> Option<similar::Algorithm> {
    match name.to_ascii_lowercase().as_str() {
        "myers" | "default" => Some(similar::Algorithm::Myers),
        "histogram" | "patience" => Some(similar::Algorithm::Patience),
        "minimal" => Some(similar::Algorithm::Lcs),
        _ => None,
    }
}

fn diff_algo_from_config_default(cfg: &grit_lib::config::ConfigSet) -> similar::Algorithm {
    cfg.get("diff.algorithm")
        .as_deref()
        .and_then(match_grit_diff_algorithm_name)
        .unwrap_or(similar::Algorithm::Myers)
}

/// Last algorithm-related flag on the argv wins (matches Git).
fn parse_cli_diff_algorithm_from_argv() -> Option<similar::Algorithm> {
    let argv: Vec<String> = std::env::args().collect();
    let mut last = None;
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].as_str();
        match a {
            "--histogram" | "--patience" => last = Some(similar::Algorithm::Patience),
            "--minimal" => last = Some(similar::Algorithm::Lcs),
            s if s.starts_with("--diff-algorithm=") => {
                let v = s.strip_prefix("--diff-algorithm=").unwrap_or("");
                last = match_grit_diff_algorithm_name(v);
            }
            "--diff-algorithm" => {
                if let Some(v) = argv.get(i + 1) {
                    last = match_grit_diff_algorithm_name(v);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    last
}

fn diff_algorithm_for_path(
    rel_path: &str,
    cli_override: Option<similar::Algorithm>,
    ctx: &DiffAlgoContext,
) -> similar::Algorithm {
    if let Some(a) = cli_override {
        return a;
    }
    let map = collect_attrs_for_path(
        &ctx.attrs.rules,
        &ctx.attrs.macros,
        rel_path,
        ctx.ignore_case_attrs,
    );
    if let Some(AttrValue::Value(driver)) = map.get("diff") {
        if let Some(algo_key) = ctx.config.get(&format!("diff.{driver}.algorithm")) {
            if let Some(a) = match_grit_diff_algorithm_name(&algo_key) {
                return a;
            }
        }
    }
    diff_algo_from_config_default(&ctx.config)
}

fn submodule_ignore_flags_from_diff_arg(ignore_sm: &str) -> SubmoduleIgnoreFlags {
    match ignore_sm {
        "all" => SubmoduleIgnoreFlags {
            ignore_all: true,
            ignore_untracked: false,
            ignore_dirty: false,
        },
        "untracked" => SubmoduleIgnoreFlags {
            ignore_all: false,
            ignore_untracked: true,
            ignore_dirty: false,
        },
        "dirty" => SubmoduleIgnoreFlags {
            ignore_all: false,
            ignore_untracked: false,
            ignore_dirty: true,
        },
        _ => SubmoduleIgnoreFlags {
            ignore_all: false,
            ignore_untracked: false,
            ignore_dirty: false,
        },
    }
}

fn write_submodule_log_lines(
    out: &mut impl Write,
    repo: &Repository,
    entry: &DiffEntry,
) -> Result<()> {
    let z = zero_oid();
    if entry.old_oid == z || entry.new_oid == z {
        return Ok(());
    }
    let old_a = abbreviate_object_id(repo, entry.old_oid, 7)?;
    let new_a = abbreviate_object_id(repo, entry.new_oid, 7)?;
    writeln!(out, "Submodule {} {}..{}:", entry.path(), old_a, new_a)?;
    let Some(wt) = repo.work_tree.as_deref() else {
        return Ok(());
    };
    let sub_path = wt.join(entry.path());
    let Ok(sub_repo) = Repository::discover(Some(&sub_path)) else {
        return Ok(());
    };
    let mut opts = RevListOptions::default();
    opts.first_parent = true;
    let Ok(res) = rev_list(
        &sub_repo,
        &[entry.new_oid.to_hex()],
        &[entry.old_oid.to_hex()],
        &opts,
    ) else {
        return Ok(());
    };
    for oid in res.commits.iter().rev() {
        let Ok(obj) = sub_repo.odb.read(oid) else {
            continue;
        };
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let Ok(c) = parse_commit(&obj.data) else {
            continue;
        };
        let subject = submodule_commit_subject_line(&c);
        writeln!(out, "  > {subject}")?;
    }
    Ok(())
}

fn submodule_commit_subject_line(c: &grit_lib::objects::CommitData) -> String {
    let enc = c.encoding.as_deref().unwrap_or("UTF-8");
    let is_latin1 = enc.eq_ignore_ascii_case("ISO8859-1")
        || enc.eq_ignore_ascii_case("ISO-8859-1")
        || enc.eq_ignore_ascii_case("LATIN1")
        || enc.eq_ignore_ascii_case("ISO-8859-15");
    if let Some(raw) = c.raw_message.as_deref() {
        let line = raw.split(|b| *b == b'\n').next().unwrap_or(raw);
        if is_latin1 {
            return line
                .iter()
                .map(|&b| b as char)
                .collect::<String>()
                .trim()
                .to_owned();
        }
        return String::from_utf8_lossy(line).trim().to_string();
    }
    c.message.lines().next().unwrap_or("").trim().to_owned()
}
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

    /// Byte-oriented line normalisation for diffing (aligned with merge three-way compare rules).
    fn normalize_line_bytes(&self, line: &[u8]) -> Vec<u8> {
        let mut bytes = line.to_vec();

        if self.ignore_cr_at_eol && bytes.ends_with(b"\r\n") {
            let len = bytes.len();
            bytes.remove(len - 2);
        }

        if self.ignore_all_space {
            return bytes
                .into_iter()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
        }

        if self.ignore_space_change {
            let mut out = Vec::with_capacity(bytes.len());
            let mut in_ws = false;
            for ch in bytes {
                if ch.is_ascii_whitespace() {
                    if !in_ws {
                        out.push(b' ');
                        in_ws = true;
                    }
                } else {
                    out.push(ch);
                    in_ws = false;
                }
            }
            while out.last().is_some_and(|b| b.is_ascii_whitespace()) {
                out.pop();
            }
            return out;
        }

        if self.ignore_space_at_eol {
            if bytes.last().copied() == Some(b'\n') {
                let mut body = bytes[..bytes.len() - 1].to_vec();
                while body.last().is_some_and(|b| b.is_ascii_whitespace()) {
                    body.pop();
                }
                body.push(b'\n');
                bytes = body;
            } else {
                while bytes.last().is_some_and(|b| b.is_ascii_whitespace()) {
                    bytes.pop();
                }
            }
        }

        bytes
    }
}

/// One logical line for `--no-index` diffing: compare key plus original bytes for output.
struct NoIndexLineSlot {
    display: Vec<u8>,
    compare: Vec<u8>,
}

/// Split blob bytes into lines, each retaining a trailing `\n` when present (Git line convention).
fn no_index_split_lines(data: &[u8]) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for i in 0..data.len() {
        if data[i] == b'\n' {
            lines.push(data[start..=i].to_vec());
            start = i + 1;
        }
    }
    if start < data.len() {
        lines.push(data[start..].to_vec());
    }
    lines
}

fn no_index_line_body_is_blank(line: &[u8]) -> bool {
    let end = if line.last().copied() == Some(b'\n') {
        line.len().saturating_sub(1)
    } else {
        line.len()
    };
    let end = if end > 0 && line.get(end - 1) == Some(&b'\r') {
        end - 1
    } else {
        end
    };
    line[..end].iter().all(|b| b.is_ascii_whitespace())
}

fn no_index_build_line_slots(data: &[u8], mode: &WhitespaceMode) -> Vec<NoIndexLineSlot> {
    no_index_split_lines(data)
        .into_iter()
        .filter(|line| !mode.ignore_blank_lines || !no_index_line_body_is_blank(line))
        .map(|line| NoIndexLineSlot {
            compare: mode.normalize_line_bytes(&line),
            display: line,
        })
        .collect()
}

/// Unified diff body (`---` / `+++` / hunks) for `--no-index`, optional algorithm and whitespace rules.
fn no_index_unified_patch_body(
    old_bytes: &[u8],
    new_bytes: &[u8],
    old_path: &str,
    new_path: &str,
    context_lines: usize,
    mode: &WhitespaceMode,
    algorithm: similar::Algorithm,
) -> String {
    let old_slots = no_index_build_line_slots(old_bytes, mode);
    let new_slots = no_index_build_line_slots(new_bytes, mode);

    // Compare keys must not include trailing `\n`: `similar`'s unified writer adds a newline
    // when `newline_terminated` is false (slice diff); a `\n` in the value would double spacing.
    let line_key = |bytes: &[u8]| -> String {
        let mut end = bytes.len();
        if end > 0 && bytes[end - 1] == b'\n' {
            end -= 1;
        }
        if end > 0 && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    };
    let old_compare_owned: Vec<String> = old_slots.iter().map(|s| line_key(&s.compare)).collect();
    let new_compare_owned: Vec<String> = new_slots.iter().map(|s| line_key(&s.compare)).collect();
    let old_compare: Vec<&str> = old_compare_owned.iter().map(|s| s.as_str()).collect();
    let new_compare: Vec<&str> = new_compare_owned.iter().map(|s| s.as_str()).collect();

    let diff = TextDiff::configure()
        .algorithm(algorithm)
        .diff_slices(&old_compare, &new_compare);

    let old_label = if old_path == "/dev/null" {
        "--- /dev/null\n".to_string()
    } else {
        format!("--- a/{old_path}\n")
    };
    let new_label = if new_path == "/dev/null" {
        "+++ /dev/null\n".to_string()
    } else {
        format!("+++ b/{new_path}\n")
    };

    let line_body_for_patch = |line: &[u8]| -> String {
        let mut end = line.len();
        if end > 0 && line[end - 1] == b'\n' {
            end -= 1;
        }
        if end > 0 && line[end - 1] == b'\r' {
            end -= 1;
        }
        String::from_utf8_lossy(&line[..end]).into_owned()
    };

    let mut output = String::new();
    output.push_str(&old_label);
    output.push_str(&new_label);

    for hunk in diff
        .unified_diff()
        .context_radius(context_lines)
        .iter_hunks()
    {
        output.push_str(&format!("{}\n", hunk.header()));
        for change in hunk.iter_changes() {
            match change.tag() {
                ChangeTag::Equal => {
                    let idx = change.new_index().expect("equal change has new_index");
                    let raw = &new_slots[idx].display;
                    output.push(' ');
                    output.push_str(&line_body_for_patch(raw));
                    output.push('\n');
                }
                ChangeTag::Delete => {
                    let idx = change.old_index().expect("delete has old_index");
                    let raw = &old_slots[idx].display;
                    output.push('-');
                    output.push_str(&line_body_for_patch(raw));
                    output.push('\n');
                }
                ChangeTag::Insert => {
                    let idx = change.new_index().expect("insert has new_index");
                    let raw = &new_slots[idx].display;
                    output.push('+');
                    output.push_str(&line_body_for_patch(raw));
                    output.push('\n');
                }
            }
        }
    }

    output
}

/// Resolve line-diff algorithm for `git diff` from flags and argv order (last wins).
fn effective_line_diff_algorithm(args: &Args) -> similar::Algorithm {
    let raw: Vec<String> = std::env::args().collect();
    let mut best: Option<(usize, similar::Algorithm)> = None;
    let mut record = |pos: Option<usize>, algo: similar::Algorithm| {
        if let Some(p) = pos {
            if best.is_none_or(|(bp, _)| p >= bp) {
                best = Some((p, algo));
            }
        }
    };
    record(
        raw.iter().rposition(|a| a == "--histogram"),
        similar::Algorithm::Patience,
    );
    record(
        raw.iter().rposition(|a| a == "--patience"),
        similar::Algorithm::Patience,
    );
    record(
        raw.iter().rposition(|a| a == "--minimal"),
        similar::Algorithm::Myers,
    );
    if let Some(name) = args.diff_algorithm.as_deref() {
        let pos = raw
            .iter()
            .rposition(|a| a == "--diff-algorithm" || a.starts_with("--diff-algorithm="));
        let algo = match name.to_lowercase().as_str() {
            "histogram" | "patience" => similar::Algorithm::Patience,
            "minimal" | "myers" => similar::Algorithm::Myers,
            _ => similar::Algorithm::Myers,
        };
        record(pos, algo);
    }
    best.map(|(_, a)| a).unwrap_or(similar::Algorithm::Myers)
}

/// Short blob id for `index` lines (matches `git rev-parse --short` default length).
fn no_index_blob_abbrev(data: &[u8]) -> String {
    let oid = Odb::hash_object_data(ObjectKind::Blob, data);
    let hex = oid.to_hex();
    let len = 7_usize.min(hex.len());
    hex[..len].to_owned()
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

    /// Omit preimage lines for deleted files (irreversible delete).
    #[arg(short = 'D', long = "irreversible-delete")]
    pub irreversible_delete: bool,

    /// Output a binary diff that can be applied with git-apply.
    #[arg(long = "binary")]
    pub binary: bool,

    /// Reverse the diff (swap old and new).
    #[arg(short = 'R')]
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

    /// Suppress all patch output (`-s` / `--no-patch`, same as Git).
    #[arg(short = 's', long = "no-patch")]
    pub no_patch: bool,

    /// Directory-level diffstat (`--dirstat=lines`, etc.). Empty value means default `changes`.
    /// Later `--dirstat` / `-X` options override earlier ones (Git semantics).
    #[arg(
        long = "dirstat",
        value_name = "PARAM",
        default_missing_value = "",
        num_args = 0..=1,
        require_equals = true,
        action = clap::ArgAction::Append
    )]
    pub dirstat: Vec<String>,

    /// Synonym for `--dirstat=files` (optional `=<param>` like `--dirstat`).
    #[arg(
        long = "dirstat-by-file",
        value_name = "PARAM",
        default_missing_value = "",
        num_args = 0..=1,
        require_equals = true
    )]
    pub dirstat_by_file: Option<String>,

    /// Set `--dirstat=cumulative` (Git synonym).
    #[arg(long = "cumulative")]
    pub dirstat_cumulative_flag: bool,

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

    /// Submodule diff output (`log` is the default for bare `--submodule`, matching Git).
    #[arg(long = "submodule", value_name = "FORMAT", default_missing_value = "log", num_args = 0..=1)]
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

    /// Redirect diff output to a file (default stdout).
    #[arg(long = "output", value_name = "file")]
    pub output_path: Option<PathBuf>,

    /// Disable rename detection (must not be abbreviated).
    #[arg(long = "no-renames")]
    pub no_renames: bool,

    /// Detect copies (treat as rename detection for now).
    #[arg(short = 'C', long = "find-copies", value_name = "N", default_missing_value = "50", num_args = 0..=1, require_equals = true)]
    pub find_copies: Option<String>,

    /// Find copies harder (look at unmodified files as source).
    #[arg(long = "find-copies-harder")]
    pub find_copies_harder: bool,

    /// Pickaxe: look for diffs that change the number of occurrences of the specified string.
    /// Parsed manually from trailing args since -S<string> value is attached.
    #[arg(skip)]
    pub pickaxe_string: Option<String>,

    /// Pickaxe: look for diffs whose patch text contains added/removed lines matching regex.
    /// Parsed manually from trailing args since -G takes a space-separated value.
    #[arg(skip)]
    pub pickaxe_grep: Option<String>,

    /// Ignore lines matching regex (`-I` / `--ignore-matching-lines`). Parsed from trailing args.
    #[arg(skip)]
    pub ignore_matching_lines: Vec<String>,

    /// Filter diff output by change type (e.g. `R` for renames only). Parsed from trailing args.
    #[arg(skip)]
    pub diff_filter: Option<String>,

    /// Treat the string given to -S as a POSIX extended regex.
    #[arg(long = "pickaxe-regex")]
    pub pickaxe_regex: bool,

    /// Commits or paths. Use `--` to separate revisions from paths.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Parsed `--dirstat` / `diff.dirstat` options (Git-compatible).
#[derive(Clone, Debug)]
struct DirstatOptions {
    /// `changes` (byte damage), `lines` (insertion+deletion lines), or `files` (1 per file).
    mode: DirstatMode,
    cumulative: bool,
    /// Minimum permille (parts per thousand) to print a line; default 30 = 3%.
    permille: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirstatMode {
    Changes,
    Lines,
    Files,
}

impl Default for DirstatOptions {
    fn default() -> Self {
        Self {
            mode: DirstatMode::Changes,
            cumulative: false,
            permille: 30,
        }
    }
}

impl DirstatOptions {
    fn is_default_everything(&self) -> bool {
        self.mode == DirstatMode::Changes && !self.cumulative && self.permille == 30
    }
}

fn parse_dirstat_apply_tokens(
    params: &str,
    opts: &mut DirstatOptions,
    strict: bool,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if params.is_empty() {
        return Ok(());
    }
    for raw in params.split(',') {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        if p.eq_ignore_ascii_case("changes") {
            opts.mode = DirstatMode::Changes;
        } else if p.eq_ignore_ascii_case("lines") {
            opts.mode = DirstatMode::Lines;
        } else if p.eq_ignore_ascii_case("files") {
            opts.mode = DirstatMode::Files;
        } else if p.eq_ignore_ascii_case("noncumulative") {
            opts.cumulative = false;
        } else if p.eq_ignore_ascii_case("cumulative") {
            opts.cumulative = true;
        } else if p.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            match parse_dirstat_percentage_permille(p) {
                Some(pm) => opts.permille = pm,
                None => {
                    let msg = format!(
                        "Failed to parse --dirstat/-X option parameter:\n  Failed to parse dirstat cut-off percentage '{p}'\n"
                    );
                    if strict {
                        bail!("{msg}");
                    }
                    warnings.push(format!(
                        "Found errors in 'diff.dirstat' config variable:\n  Failed to parse dirstat cut-off percentage '{p}'\n"
                    ));
                }
            }
        } else {
            let msg = format!(
                "Failed to parse --dirstat/-X option parameter:\n  Unknown dirstat parameter '{p}'\n"
            );
            if strict {
                bail!("{msg}");
            }
            warnings.push(format!(
                "Found errors in 'diff.dirstat' config variable:\n  Unknown dirstat parameter '{p}'\n"
            ));
        }
    }
    Ok(())
}

fn parse_dirstat_params_lenient(params: &str) -> (DirstatOptions, Vec<String>) {
    let mut o = DirstatOptions::default();
    let mut warnings = Vec::new();
    let _ = parse_dirstat_apply_tokens(params, &mut o, false, &mut warnings);
    (o, warnings)
}

#[derive(Clone, Debug)]
struct DirstatFile {
    name: String,
    changed: u64,
}

/// Write the `diff --git` line for `git diff --no-index`.
fn write_no_index_diff_git_line(out: &mut String, path_a: &str, path_b: &str) {
    let _ = writeln!(out, "diff --git a/{path_a} b/{path_b}");
}

fn abbrev_oid_hex(data: &[u8], abbrev_len: usize) -> String {
    let oid = Odb::hash_object_data(ObjectKind::Blob, data);
    let hex = oid.to_hex();
    let len = abbrev_len.min(hex.len());
    hex[..len].to_owned()
}

fn write_no_index_index_lines(
    out: &mut String,
    data_a: &[u8],
    data_b: &[u8],
    mode_a: &str,
    mode_b: &str,
    abbrev_len: usize,
) {
    let a = abbrev_oid_hex(data_a, abbrev_len);
    let b = abbrev_oid_hex(data_b, abbrev_len);
    if mode_a == mode_b {
        let _ = writeln!(out, "index {a}..{b} {mode_a}");
    } else {
        let _ = writeln!(out, "index {a}..{b}");
        let _ = writeln!(out, "old mode {mode_a}");
        let _ = writeln!(out, "new mode {mode_b}");
    }
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
        return run_no_index(args);
    }

    let repo_opt = Repository::discover(None).ok();
    let precompose_paths = repo_opt.as_ref().is_some_and(|r| {
        grit_lib::precompose_config::effective_core_precomposeunicode(Some(&r.git_dir))
    });
    if precompose_paths {
        for a in args.args.iter_mut() {
            *a = grit_lib::unicode_normalization::precompose_utf8_path(a).into_owned();
        }
    }

    let raw_args: Vec<String> = std::env::args().collect();
    let has_separator = raw_args.iter().any(|a| a == "--");
    let (mut revs, raw_path_args) =
        parse_rev_and_paths(&args.args, has_separator, precompose_paths);
    // Options parsed by clap can remain in the `revs` bucket when `--` separates paths
    // (`git diff -D -- path`). Drop duplicates so they are not treated as revision names.
    if args.irreversible_delete {
        revs.retain(|r| r != "-D" && r != "--irreversible-delete");
    }
    if args.break_rewrites {
        revs.retain(|r| r != "-B" && r != "--break-rewrites");
    }

    // Outside any repository, `git diff <path> <path>` behaves like `diff --no-index` (t4035).
    if repo_opt.is_none() && revs.is_empty() && raw_path_args.len() == 2 && !args.cached {
        return run_no_index(args);
    }

    let repo = repo_opt.context("not a git repository")?;
    let diff_config_early = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
    if args.order_file.is_none() {
        if let Some(p) = diff_config_early
            .get("diff.orderfile")
            .or_else(|| diff_config_early.get("diff.orderFile"))
        {
            let t = p.trim();
            if !t.is_empty() {
                args.order_file = Some(t.to_owned());
            }
        }
    }
    let quote_path_fully = diff_config_early.quote_path_fully();

    let patch_context = if let Some(u) = args.unified {
        u
    } else {
        let cfg = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
            .context("loading git config")?;
        grit_lib::config::resolve_diff_context_lines(&cfg)
            .map_err(|m| anyhow::anyhow!(m))?
            .unwrap_or(3)
    };

    if args.inter_hunk_context.is_none() {
        if let Ok(cfg) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true) {
            if let Some(raw) = cfg.get("diff.interhunkcontext") {
                match cfg.get_i64("diff.interhunkcontext") {
                    Some(Ok(n)) if n < 0 => {
                        anyhow::bail!("negative value for 'diff.interHunkContext': '{raw}'");
                    }
                    Some(Ok(_)) => {}
                    _ => anyhow::bail!("invalid value for 'diff.interHunkContext': '{raw}'"),
                }
            }
        }
    }

    let merged_attrs = match load_gitattributes_for_diff(&repo) {
        Ok(m) => m,
        Err(Error::InvalidRef(msg)) if msg.starts_with("bad --attr-source") => {
            eprintln!("fatal: bad --attr-source or GIT_ATTR_SOURCE");
            std::process::exit(128);
        }
        Err(e) => return Err(e.into()),
    };
    let merged_attrs = Arc::new(merged_attrs);
    let diff_config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
        .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());
    let ignore_case_attrs = diff_config
        .get("core.ignorecase")
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1"));
    let diff_algo_ctx = DiffAlgoContext {
        attrs: Arc::clone(&merged_attrs),
        config: Arc::new(diff_config.clone()),
        ignore_case_attrs,
    };
    let diff_algo_cli = parse_cli_diff_algorithm_from_argv();

    let emit_unified_patch = diff_emit_unified_patch_from_argv(&raw_args);

    let cwd = env::current_dir().context("failed to read current directory")?;
    let pathspec_prefix = repo
        .work_tree
        .as_ref()
        .map(|_| show_prefix(&repo, &cwd))
        .filter(|p| !p.is_empty())
        .map(|mut p| {
            p.pop(); // trailing '/'
            p
        });
    let paths: Vec<String> = raw_path_args
        .iter()
        .map(|p| {
            if let Some(wt) = repo.work_tree.as_ref() {
                resolve_pathspec(p, wt, pathspec_prefix.as_deref())
            } else {
                p.clone()
            }
        })
        .collect();
    // Resolve diff prefixes from config and command-line options
    let (src_prefix, dst_prefix) = resolve_diff_prefixes(&args, &repo, args.cached);

    // `git diff <rev>:<path> <file>` — compare a blob from history to a worktree file.
    if revs.len() == 1
        && paths.len() == 1
        && !args.cached
        && split_treeish_colon(&revs[0]).is_some()
    {
        return run_diff_blob_vs_file(
            &repo,
            &args,
            &revs[0],
            &paths[0],
            None::<&str>,
            &src_prefix,
            &dst_prefix,
            patch_context,
            Arc::clone(&merged_attrs),
            diff_config.clone(),
            ignore_case_attrs,
            diff_algo_cli,
            &cwd,
            quote_path_fully,
        );
    }

    // `git diff <blob-oid> <file>` — raw object id vs path (t4063: prefers filename in headers).
    if revs.len() == 1 && paths.len() == 1 && !args.cached {
        if split_treeish_colon(&revs[0]).is_none() {
            if let Some(wt) = repo.work_tree.as_ref() {
                if fs::symlink_metadata(wt.join(&paths[0])).is_ok() {
                    let oid = resolve_revision(&repo, &revs[0]).ok();
                    let is_blob = oid.is_some_and(|o| {
                        repo.odb
                            .read(&o)
                            .map(|obj| obj.kind == ObjectKind::Blob)
                            .unwrap_or(false)
                    });
                    if is_blob {
                        return run_diff_blob_vs_file(
                            &repo,
                            &args,
                            &revs[0],
                            &paths[0],
                            Some(paths[0].as_str()),
                            &src_prefix,
                            &dst_prefix,
                            patch_context,
                            Arc::clone(&merged_attrs),
                            diff_config.clone(),
                            ignore_case_attrs,
                            diff_algo_cli,
                            &cwd,
                            quote_path_fully,
                        );
                    }
                }
            }
        }
    }

    // `git diff <path> <path>` — compare a worktree file to another path (e.g. outside the repo).
    // Triggered when parse_rev_and_paths found two paths and no revisions (first token exists on disk).
    if revs.is_empty() && paths.len() == 2 && !args.cached {
        return run_diff_two_paths(&repo, &args, &paths[0], &paths[1], &src_prefix, &dst_prefix);
    }

    // Expand A...B (symmetric diff) → merge-base(A,B)..B
    // Expand A..B → A B (two-rev diff)
    // trailing_var_arg may capture flags like --name-only into args.
    // Move them back into the flags struct so they take effect.
    let mut want_combined_diff = false;
    let mut extra_revs = Vec::new();
    let mut rev_idx = 0;
    while rev_idx < revs.len() {
        let r = &revs[rev_idx];
        if r.starts_with("--") || r.starts_with("-") && r.len() > 1 {
            // Re-apply trailing flags
            match r.as_str() {
                "-c" => {
                    want_combined_diff = true;
                }
                "--name-only" => args.name_only = true,
                "--name-status" => args.name_status = true,
                "--numstat" => args.numstat = true,
                "--shortstat" => args.shortstat = true,
                "--summary" => args.summary = true,
                "--quiet" | "-q" => args.quiet = true,
                s if s.starts_with("--stat-width=") => {
                    if let Some(v) = s.strip_prefix("--stat-width=").and_then(|x| x.parse().ok()) {
                        args.stat_width = Some(v);
                    }
                    stat_enabled = true;
                }
                s if s.starts_with("--stat-name-width=") => {
                    if let Some(v) = s
                        .strip_prefix("--stat-name-width=")
                        .and_then(|x| x.parse().ok())
                    {
                        args.stat_name_width = Some(v);
                    }
                    stat_enabled = true;
                }
                s if s.starts_with("--stat-graph-width=") => {
                    if let Some(v) = s
                        .strip_prefix("--stat-graph-width=")
                        .and_then(|x| x.parse().ok())
                    {
                        args.stat_graph_width = Some(v);
                    }
                    stat_enabled = true;
                }
                s if s.starts_with("--stat-count=") => {
                    if let Some(v) = s.strip_prefix("--stat-count=").and_then(|x| x.parse().ok()) {
                        args.stat_count = Some(v);
                    }
                    stat_enabled = true;
                }
                s if s == "--stat" || s.starts_with("--stat=") => {
                    if s == "--stat" {
                        if args.stat.is_none() {
                            args.stat = Some(String::new());
                        }
                    } else if let Some(val) = s.strip_prefix("--stat=") {
                        args.stat = Some(val.to_owned());
                    }
                    if let Some(ref val) = args.stat {
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
                    }
                    stat_enabled = true;
                }
                "--exit-code" => args.exit_code = true,
                "--no-patch" => args.no_patch = true,
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
                    args.ignore_submodules = Some(
                        s.strip_prefix("--ignore-submodules=")
                            .unwrap_or("all")
                            .to_owned(),
                    );
                }
                s if s.starts_with("--color-moved") => {
                    args.color_moved = Some("default".to_owned());
                }
                s if s.starts_with("-O") && s.len() > 2 => {
                    let path = s[2..].to_string();
                    if path == "/dev/null" {
                        args.order_file = None;
                    } else {
                        args.order_file = Some(path);
                    }
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
                s if s.starts_with("--diff-filter=") => {
                    args.diff_filter =
                        Some(s.strip_prefix("--diff-filter=").unwrap_or("").to_owned());
                }
                s if s == "-M"
                    || (s.starts_with("-M")
                        && s.len() > 2
                        && s[2..].bytes().all(|b| b.is_ascii_digit() || b == b'%')) =>
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
                // `-CC` is copy detection; combined diff is spelled `--cc` only (Git).
                s if s == "-C" || s.starts_with("--find-copies") => {
                    args.find_copies = Some("50".to_owned());
                }
                "--cc" => {
                    want_combined_diff = true;
                }
                "--find-copies-harder" => {
                    args.find_copies_harder = true;
                    args.find_copies = Some("50".to_owned());
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
                "-I" | "--ignore-matching-lines" => {
                    if rev_idx + 1 < revs.len() {
                        rev_idx += 1;
                        args.ignore_matching_lines.push(revs[rev_idx].clone());
                    }
                }
                s if s.starts_with("-I") && s.len() > 2 => {
                    args.ignore_matching_lines.push(s[2..].to_owned());
                }
                s if s.starts_with("--ignore-matching-lines=") => {
                    args.ignore_matching_lines.push(
                        s.strip_prefix("--ignore-matching-lines=")
                            .unwrap_or("")
                            .to_owned(),
                    );
                }
                "--pickaxe-regex" => {
                    args.pickaxe_regex = true;
                }
                "--pickaxe-all" => {
                    // Accepted for compatibility
                }
                s if s == "--dirstat" => {
                    args.dirstat.push(String::new());
                }
                s if s.starts_with("--dirstat=") => {
                    args.dirstat
                        .push(s.strip_prefix("--dirstat=").unwrap_or("").to_owned());
                }
                s if s == "--dirstat-by-file" => {
                    args.dirstat_by_file = Some(String::new());
                }
                s if s.starts_with("--dirstat-by-file=") => {
                    args.dirstat_by_file = Some(
                        s.strip_prefix("--dirstat-by-file=")
                            .unwrap_or("")
                            .to_owned(),
                    );
                }
                "--cumulative" => {
                    args.dirstat_cumulative_flag = true;
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
    if revs.len() == 3 && args.name_only && !args.cached {
        want_combined_diff = true;
    }

    let mut _symmetric = false;
    if revs.len() == 1 {
        if let Some((left_spec, right_spec)) = try_treeish_blob_range(&revs[0]) {
            revs = vec![left_spec, right_spec];
        } else if let Some((left, right)) = revs[0].split_once("...") {
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
    let index = match repo.load_index() {
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
        (_, 3) if want_combined_diff && args.name_only && !args.cached => None,
        _ => None,
    };

    let entries: Vec<DiffEntry> = if want_combined_diff
        && revs.len() == 3
        && args.name_only
        && !args.cached
    {
        let merge_oid = resolve_revision(&repo, &revs[0])
            .with_context(|| format!("unknown revision: '{}'", revs[0]))?;
        let p_a = resolve_revision(&repo, &revs[1])
            .with_context(|| format!("unknown revision: '{}'", revs[1]))?;
        let p_b = resolve_revision(&repo, &revs[2])
            .with_context(|| format!("unknown revision: '{}'", revs[2]))?;
        let merge_obj = repo
            .odb
            .read(&merge_oid)
            .with_context(|| format!("reading object {merge_oid}"))?;
        if merge_obj.kind != ObjectKind::Commit {
            bail!("combined diff requires a merge commit");
        }
        let merge_commit = parse_commit(&merge_obj.data).context("parsing merge commit")?;
        if merge_commit.parents.len() != 2 {
            bail!("combined diff requires a merge commit with exactly two parents");
        }
        let parents_ok = merge_commit.parents == [p_a, p_b] || merge_commit.parents == [p_b, p_a];
        if !parents_ok {
            bail!("combined diff: revisions do not match merge parents");
        }
        let names = combined_diff_paths(&repo.odb, &merge_commit.tree, &[p_a, p_b]);
        let z = zero_oid();
        names
            .into_iter()
            .map(|p| DiffEntry {
                status: DiffStatus::Modified,
                old_path: Some(p.clone()),
                new_path: Some(p),
                old_mode: "100644".to_string(),
                new_mode: "100644".to_string(),
                old_oid: z,
                new_oid: z,
                score: None,
            })
            .collect()
    } else {
        match (args.cached, revs.len()) {
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
                match (
                    blob_side_for_blob_diff_spec(&repo, &revs[0])?,
                    blob_side_for_blob_diff_spec(&repo, &revs[1])?,
                ) {
                    (Some(a), Some(b)) => {
                        vec![DiffEntry {
                            status: DiffStatus::Modified,
                            old_path: Some(a.path),
                            new_path: Some(b.path),
                            old_mode: a.mode,
                            new_mode: b.mode,
                            old_oid: a.oid,
                            new_oid: b.oid,
                            score: None,
                        }]
                    }
                    _ => {
                        let tree1 = commit_or_tree_oid(&repo, &revs[0])?;
                        let tree2 = commit_or_tree_oid(&repo, &revs[1])?;
                        diff_trees(&repo.odb, Some(&tree1), Some(&tree2), "")?
                    }
                }
            }
            _ => {
                bail!("too many revisions");
            }
        }
    };

    // Filter by pathspecs
    let entries = filter_by_paths(entries, &raw_path_args);

    // Build whitespace mode from flags
    let ws_mode = WhitespaceMode {
        ignore_all_space: args.ignore_all_space,
        ignore_space_change: args.ignore_space_change,
        ignore_space_at_eol: args.ignore_space_at_eol,
        ignore_blank_lines: args.ignore_blank_lines,
        ignore_cr_at_eol: args.ignore_cr_at_eol,
    };

    let line_ignore_res: Option<Vec<Regex>> = if args.ignore_matching_lines.is_empty() {
        None
    } else {
        let mut compiled = Vec::with_capacity(args.ignore_matching_lines.len());
        for p in &args.ignore_matching_lines {
            match Regex::new(p) {
                Ok(re) => compiled.push(re),
                Err(_) => {
                    eprintln!("error: invalid regex given to -I: {p}");
                    std::process::exit(129);
                }
            }
        }
        Some(compiled)
    };
    let line_ignore = line_ignore_res.as_deref();

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

    let entries = if let Some(ign) = line_ignore {
        entries
            .into_iter()
            .filter(|e| !entry_hidden_by_line_ignore(e, &repo.odb, wt_for_content, &ws_mode, ign))
            .collect()
    } else {
        entries
    };

    // -C implies -M (copy detection requires rename detection)
    if args.find_copies.is_some() && args.find_renames.is_none() {
        args.find_renames = Some("50".to_owned());
    }

    // Apply rename detection if requested (explicit -M flag or diff.renames config).
    let rename_threshold: Option<u32> = if let Some(ref threshold_str) = args.find_renames {
        Some(parse_diff_rename_threshold(threshold_str))
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

    // `--ignore-submodules=all` hides gitlink paths entirely. `dirty` / `untracked` still show
    // when the superproject records a different submodule commit (tree-to-tree / index gitlink
    // updates); they only suppress "submodule working tree dirty" noise elsewhere (matches Git;
    // see `t4137-apply-submodule.sh`).
    let ignore_sm = args.ignore_submodules.as_deref().unwrap_or("none");
    let entries = if ignore_sm == "all" {
        entries
            .into_iter()
            .filter(|e| e.old_mode != "160000" && e.new_mode != "160000")
            .collect()
    } else {
        entries
    };

    let entries = if let Some(ref df) = args.diff_filter {
        if df.is_empty() {
            entries
        } else {
            apply_diff_filter(entries, df)
        }
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
    let entries = {
        let prefix = resolve_diff_relative_prefix(work_tree, &repo.git_dir, &args);
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
    };

    // Apply orderfile sorting if specified
    let entries = if let Some(ref order_path) = args.order_file {
        apply_orderfile(entries, order_path, &cwd)?
    } else {
        entries
    };

    let entries =
        apply_rotate_skip_entries(entries, args.rotate_to.as_deref(), args.skip_to.as_deref())?;

    // Apply -R: reverse the diff (swap old and new sides)
    let mut entries = if args.reverse {
        entries
            .into_iter()
            .map(|mut e| {
                std::mem::swap(&mut e.old_oid, &mut e.new_oid);
                std::mem::swap(&mut e.old_mode, &mut e.new_mode);
                // Swap paths for every entry, including additions/deletions where one side
                // is `None`. The previous `if let Some(old_path)` branch skipped Added files
                // and produced invalid patches (`--- /dev/null` + `+++ /dev/null`).
                std::mem::swap(&mut e.old_path, &mut e.new_path);
                // Invert the status
                e.status = match e.status {
                    grit_lib::diff::DiffStatus::Added => grit_lib::diff::DiffStatus::Deleted,
                    grit_lib::diff::DiffStatus::Deleted => grit_lib::diff::DiffStatus::Added,
                    other => other,
                };
                e
            })
            .collect()
    } else {
        entries
    };

    if args.break_rewrites {
        for entry in &mut entries {
            if entry.status != DiffStatus::Modified {
                continue;
            }
            let old_raw = read_content_raw(&repo.odb, &entry.old_oid);
            let new_raw = read_content_raw_or_worktree(
                &repo.odb,
                &entry.new_oid,
                wt_for_content,
                entry.path(),
            );
            if is_binary(&old_raw) || is_binary(&new_raw) {
                continue;
            }
            if should_break_rewrite_for_stat(&old_raw, &new_raw) {
                if let Some(pct) = rewrite_dissimilarity_index_percent(&old_raw, &new_raw) {
                    entry.score = Some(pct);
                }
            }
        }
    }

    let dirstat_cli_active = !args.dirstat.is_empty() || args.dirstat_by_file.is_some();
    let (dirstat_opts, dirstat_config_warnings) =
        resolve_dirstat_options(&args, &repo.git_dir, dirstat_cli_active)?;
    let relative_prefix_for_paths = resolve_diff_relative_prefix(work_tree, &repo.git_dir, &args);
    let format_besides_unified_patch = args.shortstat
        || stat_enabled
        || args.stat_count.is_some()
        || args.stat_width.is_some()
        || args.stat_graph_width.is_some()
        || args.stat_name_width.is_some()
        || args.raw
        || args.numstat
        || args.name_only
        || args.name_status
        || (args.summary && !stat_enabled)
        || dirstat_opts.is_some();

    let merge_in_progress = std::fs::metadata(repo.git_dir.join("MERGE_HEAD")).is_ok();
    let mut conflict_combined_patches: Vec<String> = Vec::new();
    if merge_in_progress && !args.cached && revs.is_empty() && work_tree.is_some() {
        let mut conflict_paths: Vec<String> = entries
            .iter()
            .filter(|e| e.status == DiffStatus::Unmerged)
            .map(|e| e.path().to_string())
            .collect();
        conflict_paths.sort();
        conflict_paths.dedup();
        if !conflict_paths.is_empty() {
            use grit_lib::config::ConfigSet;
            let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
            let patch_abbrev = if args.full_index {
                40usize
            } else if let Some(n) = args.abbrev {
                n.max(4).min(40)
            } else {
                7
            };
            if let Some(wt) = work_tree {
                for display_path in &conflict_paths {
                    let repo_path = repo_relative_path_for_relative_display(
                        display_path,
                        relative_prefix_for_paths.as_deref(),
                    );
                    let key = repo_path.as_bytes();
                    let Some(e1) = index.get(key, 1) else {
                        continue;
                    };
                    let Some(e2) = index.get(key, 2) else {
                        continue;
                    };
                    let Some(e3) = index.get(key, 3) else {
                        continue;
                    };
                    let file_path = wt.join(&repo_path);
                    let wt_bytes = std::fs::read(&file_path).unwrap_or_default();
                    conflict_combined_patches.push(format_worktree_conflict_combined(
                        &repo.git_dir,
                        &config,
                        &repo.odb,
                        display_path,
                        &e1.oid,
                        &e2.oid,
                        &e3.oid,
                        &wt_bytes,
                        patch_abbrev,
                    ));
                }
            }
            // Git keeps index↔worktree `U`/`M` lines for `--raw` / `--name-only` during conflicts;
            // combined `diff --cc` replaces them only when unified patch is the sole format.
            let strip_conflict_index_lines =
                emit_unified_patch && !args.no_patch && !format_besides_unified_patch;
            if strip_conflict_index_lines {
                entries.retain(|e| {
                    if conflict_paths.iter().any(|p| p == e.path()) {
                        e.status != DiffStatus::Unmerged && e.status != DiffStatus::Modified
                    } else {
                        true
                    }
                });
            }
        }
    }

    let has_diff = !entries.is_empty() || !conflict_combined_patches.is_empty();

    // Determine color mode
    let use_color = match args.color.as_deref() {
        Some("always") => true,
        Some("never") => false,
        Some("auto") | None => {
            if args.output_path.is_some() {
                false
            } else {
                io::stdout().is_terminal()
            }
        }
        Some(_) => false,
    };

    let mut out: Box<dyn Write> = if let Some(ref p) = args.output_path {
        let resolved = if p.is_absolute() {
            p.clone()
        } else {
            cwd.join(p)
        };
        Box::new(
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&resolved)
                .with_context(|| format!("failed to open output file {}", resolved.display()))?,
        )
    } else {
        Box::new(io::stdout())
    };

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

    // `--check`: whitespace / conflict-marker diagnostics. With `--exit-code`, Git uses 1 for a
    // diff that passes the check and 3 when the check fails; without `--exit-code`, a failed
    // check exits 2 (see t4017-diff-retval).
    if args.check {
        let attr_rules: Option<Vec<AttrRule>> = wt_for_content.map(load_gitattributes);
        let config_for_attrs = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let has_ws_errors = check_whitespace_errors(
            &mut out,
            &entries,
            &repo.odb,
            wt_for_content,
            attr_rules.as_deref(),
            &config_for_attrs,
        )?;
        if has_ws_errors {
            if args.exit_code {
                std::process::exit(3);
            }
            std::process::exit(2);
        }
        if args.exit_code || args.quiet {
            if has_diff {
                std::process::exit(1);
            }
            return Ok(());
        }
        return Ok(());
    }

    // `--quiet` alone suppresses stdout, but it still must honor `--exit-code` (below). `-s` /
    // `--no-patch` suppresses the unified patch without implying `--quiet`.
    let quiet_suppresses_stdout = args.quiet && !format_besides_unified_patch;

    for w in &dirstat_config_warnings {
        eprintln!("warning: {w}");
    }

    if !quiet_suppresses_stdout {
        let context_lines = patch_context;
        let inter_hunk_context = args
            .inter_hunk_context
            .or_else(|| {
                grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
                    .ok()
                    .and_then(|cfg| cfg.get("diff.interhunkcontext"))
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0);
        if args.shortstat {
            write_shortstat(
                &mut out,
                &entries,
                &repo.odb,
                wt_for_content,
                args.break_rewrites,
                line_ignore,
                &ws_mode,
                &diff_algo_ctx,
                diff_algo_cli,
            )?;
            if let Some(ref ds) = dirstat_opts {
                write_dirstat(
                    &mut out,
                    ds,
                    &entries,
                    &repo.odb,
                    wt_for_content,
                    args.break_rewrites,
                )?;
            }
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
                args.break_rewrites,
                args.stat_graph_width,
                args.line_prefix.as_deref().unwrap_or(""),
                &repo.git_dir,
                line_ignore,
                &ws_mode,
                quote_path_fully,
                &diff_algo_ctx,
                diff_algo_cli,
            )?;
            if args.summary {
                write_diff_summary(&mut out, &entries, args.break_rewrites, quote_path_fully)?;
            }
            // `git diff --stat -p` prints the stat summary and the unified patch (Git last-flag-wins
            // for `-p` vs `-s`; combined `-sp` is different).
            let show_unified_after_stat = emit_unified_patch && !args.no_patch;
            if show_unified_after_stat {
                for patch in &conflict_combined_patches {
                    write!(out, "{patch}")?;
                }
                let patch_abbrev = if args.full_index {
                    40usize
                } else if let Some(n) = args.abbrev {
                    n.max(4).min(40)
                } else {
                    7
                };
                let diff_config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
                    .unwrap_or_default();
                let external_diff_cmd = std::env::var("GIT_EXTERNAL_DIFF")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        diff_config
                            .get("diff.external")
                            .filter(|s| !s.trim().is_empty())
                    });
                write_patch_with_prefix(
                    &mut out,
                    &repo,
                    &entries,
                    &repo.odb,
                    &repo.git_dir,
                    &diff_config,
                    context_lines,
                    use_color,
                    word_diff,
                    wt_for_content,
                    suppress_blank_empty,
                    patch_abbrev,
                    inter_hunk_context,
                    args.binary,
                    args.break_rewrites,
                    args.irreversible_delete,
                    !args.no_textconv,
                    &src_prefix,
                    &dst_prefix,
                    args.submodule.as_deref(),
                    submodule_ignore_flags_from_diff_arg(ignore_sm),
                    line_ignore,
                    &diff_algo_ctx,
                    diff_algo_cli,
                    args.cached,
                    args.no_ext_diff,
                    external_diff_cmd.as_deref(),
                    relative_prefix_for_paths.as_deref(),
                )?;
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
            write_numstat(
                &mut out,
                &entries,
                &repo.odb,
                wt_for_content,
                args.break_rewrites,
                line_ignore,
                &ws_mode,
                &diff_algo_ctx,
                diff_algo_cli,
            )?;
        } else if args.name_only {
            write_name_only(&mut out, &entries, quote_path_fully)?;
        } else if args.name_status {
            write_name_status(&mut out, &entries, quote_path_fully)?;
        } else if args.summary && !stat_enabled {
            write_diff_summary(&mut out, &entries, args.break_rewrites, quote_path_fully)?;
        } else {
            let patch_abbrev = if args.full_index {
                40
            } else if let Some(n) = args.abbrev {
                n.max(4).min(40)
            } else {
                7
            };
            let show_unified = emit_unified_patch && !args.no_patch;
            if show_unified {
                for patch in &conflict_combined_patches {
                    write!(out, "{patch}")?;
                }
            }
            if let Some(ref ds) = dirstat_opts {
                write_dirstat(
                    &mut out,
                    ds,
                    &entries,
                    &repo.odb,
                    wt_for_content,
                    args.break_rewrites,
                )?;
            }
            if show_unified {
                let diff_config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
                    .unwrap_or_default();
                let external_diff_cmd = std::env::var("GIT_EXTERNAL_DIFF")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        diff_config
                            .get("diff.external")
                            .filter(|s| !s.trim().is_empty())
                    });
                write_patch_with_prefix(
                    &mut out,
                    &repo,
                    &entries,
                    &repo.odb,
                    &repo.git_dir,
                    &diff_config,
                    context_lines,
                    use_color,
                    word_diff,
                    wt_for_content,
                    suppress_blank_empty,
                    patch_abbrev,
                    inter_hunk_context,
                    args.binary,
                    args.break_rewrites,
                    args.irreversible_delete,
                    !args.no_textconv,
                    &src_prefix,
                    &dst_prefix,
                    args.submodule.as_deref(),
                    submodule_ignore_flags_from_diff_arg(ignore_sm),
                    line_ignore,
                    &diff_algo_ctx,
                    diff_algo_cli,
                    args.cached,
                    args.no_ext_diff,
                    external_diff_cmd.as_deref(),
                    relative_prefix_for_paths.as_deref(),
                )?;
            }
        }
    }

    if (args.exit_code || args.quiet) && has_diff {
        std::process::exit(1);
    }

    Ok(())
}

/// `git diff <rev>:<path> <file>` — blob at revision vs worktree file (t4063-diff-blobs).
fn run_diff_blob_vs_file(
    repo: &Repository,
    args: &Args,
    rev_path: &str,
    file_path: &str,
    display_old_blob_as: Option<&str>,
    src_prefix: &str,
    dst_prefix: &str,
    patch_context: usize,
    merged_attrs: Arc<grit_lib::attributes::ParsedGitAttributes>,
    diff_config: grit_lib::config::ConfigSet,
    ignore_case_attrs: bool,
    diff_algo_cli: Option<similar::Algorithm>,
    cwd: &Path,
    quote_path_fully: bool,
) -> Result<()> {
    let wt = repo
        .work_tree
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;
    let abs = wt.join(file_path);

    let (old_path, old_mode, old_oid) = if split_treeish_colon(rev_path).is_some() {
        let tree_side = resolve_treeish_blob_at_path(repo, rev_path)
            .with_context(|| format!("unknown revision: '{rev_path}'"))?;
        (tree_side.path, tree_side.mode, tree_side.oid)
    } else {
        let oid = resolve_revision(repo, rev_path)
            .with_context(|| format!("unknown revision: '{rev_path}'"))?;
        let obj = repo
            .odb
            .read(&oid)
            .with_context(|| format!("reading object {oid}"))?;
        if obj.kind != ObjectKind::Blob {
            bail!("object '{oid}' does not name a blob");
        }
        let label = display_old_blob_as
            .map(str::to_owned)
            .unwrap_or_else(|| oid.to_hex());
        (label, "100644".to_owned(), oid)
    };

    let wt_mode = worktree_file_mode_octal(&abs);
    let disk_oid = Odb::hash_object_data(ObjectKind::Blob, &fs::read(&abs).unwrap_or_default());

    let entries = vec![DiffEntry {
        status: DiffStatus::Modified,
        old_path: Some(old_path),
        new_path: Some(file_path.to_owned()),
        old_mode,
        new_mode: wt_mode,
        old_oid,
        new_oid: disk_oid,
        score: None,
    }];
    let entry = &entries[0];

    let has_diff = entry.old_oid != entry.new_oid || entry.old_mode != entry.new_mode;
    if !has_diff {
        return Ok(());
    }

    if args.check {
        let mut out: Box<dyn Write> = if let Some(ref p) = args.output_path {
            let resolved = if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            };
            Box::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&resolved)
                    .with_context(|| {
                        format!("failed to open output file {}", resolved.display())
                    })?,
            )
        } else {
            Box::new(io::stdout())
        };
        let attr_rules: Option<Vec<AttrRule>> = Some(load_gitattributes(&repo.git_dir));
        let config_for_attrs = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let has_ws_errors = check_whitespace_errors(
            &mut out,
            &entries,
            &repo.odb,
            Some(wt.as_ref()),
            attr_rules.as_deref(),
            &config_for_attrs,
        )?;
        if has_ws_errors {
            if args.exit_code {
                std::process::exit(3);
            }
            std::process::exit(2);
        }
        if args.exit_code || args.quiet {
            std::process::exit(1);
        }
        return Ok(());
    }

    let diff_algo_ctx = DiffAlgoContext {
        attrs: merged_attrs,
        config: Arc::new(diff_config.clone()),
        ignore_case_attrs,
    };
    let inter_hunk_context = args
        .inter_hunk_context
        .or_else(|| {
            diff_config
                .get("diff.interhunkcontext")
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0);
    let patch_abbrev = if args.full_index {
        40usize
    } else if let Some(n) = args.abbrev {
        n.max(4).min(40)
    } else {
        7
    };
    let use_color = match args.color.as_deref() {
        Some("always") => true,
        Some("never") => false,
        Some("auto") | None => {
            if args.output_path.is_some() {
                false
            } else {
                io::stdout().is_terminal()
            }
        }
        Some(_) => false,
    };
    let suppress_blank_empty = diff_config
        .get("diff.suppressBlankEmpty")
        .map(|v| v == "true")
        .unwrap_or(false);

    let external_diff_cmd = std::env::var("GIT_EXTERNAL_DIFF")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            diff_config
                .get("diff.external")
                .filter(|s| !s.trim().is_empty())
        });
    let relative_prefix_for_paths =
        resolve_diff_relative_prefix(Some(wt.as_ref()), &repo.git_dir, args);

    let mut out: Box<dyn Write> = if let Some(ref p) = args.output_path {
        let resolved = if p.is_absolute() {
            p.clone()
        } else {
            cwd.join(p)
        };
        Box::new(
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&resolved)
                .with_context(|| format!("failed to open output file {}", resolved.display()))?,
        )
    } else {
        Box::new(io::stdout())
    };

    let word_diff = args.word_diff.is_some() || args.color_words;
    let show_patch = !args.quiet && !args.no_patch;
    if show_patch {
        write_patch_with_prefix(
            &mut out,
            repo,
            &entries,
            &repo.odb,
            &repo.git_dir,
            &diff_config,
            patch_context,
            use_color,
            word_diff,
            Some(wt.as_ref()),
            suppress_blank_empty,
            patch_abbrev,
            inter_hunk_context,
            args.binary,
            args.break_rewrites,
            args.irreversible_delete,
            !args.no_textconv,
            src_prefix,
            dst_prefix,
            args.submodule.as_deref(),
            submodule_ignore_flags_from_diff_arg(
                args.ignore_submodules.as_deref().unwrap_or("none"),
            ),
            None,
            &diff_algo_ctx,
            diff_algo_cli,
            false,
            args.no_ext_diff,
            external_diff_cmd.as_deref(),
            relative_prefix_for_paths.as_deref(),
        )?;
    }

    if args.summary {
        write_diff_summary(&mut out, &entries, args.break_rewrites, quote_path_fully)?;
    }

    if (args.exit_code || args.quiet) && has_diff {
        std::process::exit(1);
    }
    Ok(())
}

/// Compare a path relative to the repository work tree to a second path (often outside the repo).
///
/// This matches `git diff <in-repo-path> <other-path>` when both arguments exist on disk and are
/// not revision specs.
fn run_diff_two_paths(
    repo: &Repository,
    args: &Args,
    path_in_repo: &str,
    path_other: &str,
    src_prefix: &str,
    dst_prefix: &str,
) -> Result<()> {
    let wt = repo
        .work_tree
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let read_path_or_symlink = |p: &Path, name: &str| -> Result<Vec<u8>> {
        if let Ok(meta) = std::fs::symlink_metadata(p) {
            if meta.file_type().is_symlink() {
                return std::fs::read_link(p)
                    .map(|target| target.to_string_lossy().into_owned().into_bytes())
                    .with_context(|| format!("could not read symlink '{name}'"));
            }
        }
        std::fs::read(p).with_context(|| format!("could not read '{name}'"))
    };

    let abs_in_repo = wt.join(path_in_repo);
    let other = Path::new(path_other);
    let abs_other = if other.is_absolute() {
        other.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| wt.to_path_buf())
            .join(other)
    };

    let data_a = read_path_or_symlink(&abs_in_repo, path_in_repo)?;
    let data_b = read_path_or_symlink(&abs_other, path_other)?;

    let text_a = String::from_utf8_lossy(&data_a);
    let text_b = String::from_utf8_lossy(&data_b);

    let ws_mode = WhitespaceMode {
        ignore_all_space: args.ignore_all_space,
        ignore_space_change: args.ignore_space_change,
        ignore_space_at_eol: args.ignore_space_at_eol,
        ignore_blank_lines: args.ignore_blank_lines,
        ignore_cr_at_eol: args.ignore_cr_at_eol,
    };

    let mut has_diff = text_a != text_b;
    if has_diff && ws_mode.any() && ws_mode.normalize(&text_a) == ws_mode.normalize(&text_b) {
        has_diff = false;
    }

    if !has_diff {
        return Ok(());
    }

    if args.quiet {
        std::process::exit(1);
    }

    let context_lines = if let Some(u) = args.unified {
        u
    } else {
        grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)
            .ok()
            .and_then(|cfg| cfg.get("diff.context"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    };

    let old_label = format!("{src_prefix}{path_in_repo}");
    let new_label = format!("{dst_prefix}{path_other}");
    let patch = unified_diff(
        text_a.as_ref(),
        text_b.as_ref(),
        &old_label,
        &new_label,
        context_lines,
    );

    let mut out = io::stdout().lock();
    let show_patch = !args.no_patch;
    if show_patch {
        let use_color = match args.color.as_deref() {
            Some("always") => true,
            Some("never") => false,
            Some("auto") | None => io::stdout().is_terminal(),
            Some(_) => false,
        };
        if use_color {
            for line in patch.lines() {
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
            write!(out, "{patch}")?;
        }
    }

    if (args.exit_code || args.quiet) && has_diff {
        std::process::exit(1);
    }
    Ok(())
}

fn no_index_attr_rules(config: &grit_lib::config::ConfigSet) -> Vec<grit_lib::crlf::AttrRule> {
    let mut rules = Vec::new();
    if let Some(p) = config.get("core.attributesfile") {
        let path = Path::new(p.trim());
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        if let Ok(content) = fs::read_to_string(&path) {
            rules.extend(parse_gitattributes_content(&content));
        }
    }
    rules
}

fn no_index_apply_textconv(
    raw: &[u8],
    path_display: &str,
    rules: &[grit_lib::crlf::AttrRule],
    config: &grit_lib::config::ConfigSet,
    command_cwd: &Path,
) -> String {
    let fa = get_file_attrs(rules, path_display, false, config);
    let DiffAttr::Driver(ref driver) = fa.diff_attr else {
        return String::from_utf8_lossy(raw).into_owned();
    };
    if grit_lib::merge_diff::diff_textconv_cmd_line(config, driver).is_none() {
        return String::from_utf8_lossy(raw).into_owned();
    }
    run_textconv(command_cwd, config, driver, raw)
        .unwrap_or_else(|| String::from_utf8_lossy(raw).into_owned())
}

/// Split args on `--` to separate revisions from paths.
///
/// Run `diff --no-index <path_a> <path_b>` — compare two files outside a repo.
fn run_no_index(args: Args) -> Result<()> {
    // Collect paths (skip "--" separators and unrecognized flags)
    let paths: Vec<&String> = args
        .args
        .iter()
        .filter(|a| a.as_str() != "--" && !a.starts_with('-'))
        .collect();
    if paths.len() != 2 {
        bail!("diff --no-index requires exactly two paths");
    }

    let path_a_str = paths[0].as_str().to_owned();
    let path_b_str = paths[1].as_str().to_owned();

    let path_a = Path::new(path_a_str.as_str());
    let path_b = Path::new(path_b_str.as_str());

    // If both paths are directories, diff all files recursively
    if path_a.is_dir() && path_b.is_dir() {
        return run_no_index_dirs(args, path_a, path_b);
    }

    let repo_opt = Repository::discover(None).ok();
    let merged_attrs = if let Some(ref r) = repo_opt {
        match load_gitattributes_for_diff(r) {
            Ok(m) => m,
            Err(Error::InvalidRef(msg)) if msg.starts_with("bad --attr-source") => {
                eprintln!("fatal: bad --attr-source or GIT_ATTR_SOURCE");
                std::process::exit(128);
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        grit_lib::attributes::ParsedGitAttributes::default()
    };
    let diff_config = repo_opt
        .as_ref()
        .map(|r| {
            grit_lib::config::ConfigSet::load(Some(&r.git_dir), true)
                .unwrap_or_else(|_| grit_lib::config::ConfigSet::new())
        })
        .unwrap_or_default();
    let ignore_case_attrs = diff_config
        .get("core.ignorecase")
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1"));
    let diff_algo_ctx = DiffAlgoContext {
        attrs: Arc::new(merged_attrs),
        config: Arc::new(diff_config),
        ignore_case_attrs,
    };
    let diff_algo_cli = parse_cli_diff_algorithm_from_argv();

    // Read file or symlink target (for symlinks, read the target path as content)
    let read_path_or_symlink = |p: &Path, name: &str| -> Result<Vec<u8>> {
        if let Ok(meta) = std::fs::symlink_metadata(p) {
            if meta.file_type().is_symlink() {
                return std::fs::read_link(p)
                    .map(|target| target.to_string_lossy().into_owned().into_bytes())
                    .with_context(|| format!("could not read symlink '{}'", name));
            }
        }
        std::fs::read(p).with_context(|| format!("could not read '{}'", name))
    };
    let data_a = read_path_or_symlink(path_a, &path_a_str)?;
    let data_b = read_path_or_symlink(path_b, &path_b_str)?;

    let config = grit_lib::config::ConfigSet::load(None, true).unwrap_or_default();
    let attr_rules = no_index_attr_rules(&config);
    let textconv_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let text_a = no_index_apply_textconv(
        &data_a,
        paths[0].as_str(),
        &attr_rules,
        &config,
        &textconv_cwd,
    );
    let text_b = no_index_apply_textconv(
        &data_b,
        paths[1].as_str(),
        &attr_rules,
        &config,
        &textconv_cwd,
    );

    if text_a == text_b {
        return Ok(());
    }

    let ws_mode = WhitespaceMode {
        ignore_all_space: args.ignore_all_space,
        ignore_space_change: args.ignore_space_change,
        ignore_space_at_eol: args.ignore_space_at_eol,
        ignore_blank_lines: args.ignore_blank_lines,
        ignore_cr_at_eol: args.ignore_cr_at_eol,
    };

    if ws_mode.any() && ws_mode.normalize(&text_a) == ws_mode.normalize(&text_b) {
        return Ok(());
    }

    if args.quiet {
        std::process::exit(1);
    }
    let context_lines = args.unified.unwrap_or(3);
    let patch_abbrev = if args.full_index {
        40usize
    } else if let Some(n) = args.abbrev {
        n.max(4).min(40)
    } else {
        7
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let algo_for_paths = |rel_a: &str, rel_b: &str| -> similar::Algorithm {
        let a = diff_algorithm_for_path(rel_a, diff_algo_cli, &diff_algo_ctx);
        let b = diff_algorithm_for_path(rel_b, diff_algo_cli, &diff_algo_ctx);
        if a == b {
            a
        } else {
            diff_algo_from_config_default(&diff_algo_ctx.config)
        }
    };

    if args.name_only {
        writeln!(out, "{path_b_str}")?;
        std::process::exit(1);
    }

    if args.name_status {
        writeln!(out, "M\t{path_b_str}")?;
        std::process::exit(1);
    }

    if args.numstat {
        let algo = algo_for_paths(path_a_str.as_str(), path_b_str.as_str());
        let (adds, dels) = count_changes_with_algorithm(&text_a, &text_b, algo);
        writeln!(out, "{}\t{}\t{}", adds, dels, path_b_str)?;
        std::process::exit(1);
    }

    if args.stat.is_some() || args.shortstat {
        let algo = algo_for_paths(path_a_str.as_str(), path_b_str.as_str());
        let (adds, dels) = count_changes_with_algorithm(&text_a, &text_b, algo);
        if args.stat.is_some() {
            let display = if path_a_str != path_b_str {
                format!("{path_a_str} => {path_b_str}")
            } else {
                path_a_str.clone()
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

    let use_anchored = if !args.anchored.is_empty() {
        let raw_args: Vec<String> = std::env::args().collect();
        let last_anchored_pos = raw_args.iter().rposition(|a| a.starts_with("--anchored"));
        let last_other_algo_pos = raw_args.iter().rposition(|a| {
            a == "--patience"
                || a == "--histogram"
                || a == "--minimal"
                || a.starts_with("--diff-algorithm")
        });
        match (last_anchored_pos, last_other_algo_pos) {
            (Some(a), Some(o)) => a > o,
            (Some(_), None) => true,
            _ => false,
        }
    } else {
        false
    };

    let line_algo_anchored = effective_line_diff_algorithm(&args);
    let algo_no_index = parse_cli_diff_algorithm_from_argv()
        .unwrap_or_else(|| algo_for_paths(path_a_str.as_str(), path_b_str.as_str()));

    let diff_body = if use_anchored {
        anchored_unified_diff(
            &text_a,
            &text_b,
            &path_a_str,
            &path_b_str,
            context_lines,
            &args.anchored,
            line_algo_anchored,
        )
    } else {
        no_index_unified_patch_body(
            &data_a,
            &data_b,
            paths[0].as_str(),
            paths[1].as_str(),
            context_lines,
            &ws_mode,
            algo_no_index,
        )
    };

    let old_abbrev = abbrev_oid_hex(&data_a, patch_abbrev);
    let new_abbrev = abbrev_oid_hex(&data_b, patch_abbrev);
    let mode_str = if path_a
        .symlink_metadata()
        .ok()
        .is_some_and(|m| m.file_type().is_symlink())
        || path_b
            .symlink_metadata()
            .ok()
            .is_some_and(|m| m.file_type().is_symlink())
    {
        "120000"
    } else {
        "100644"
    };

    let use_color = match args.color.as_deref() {
        Some("always") => true,
        Some("never") => false,
        Some("auto") | None => io::stdout().is_terminal(),
        Some(_) => false,
    };

    if use_color {
        writeln!(out, "{BOLD}diff --git a/{} b/{}{RESET}", paths[0], paths[1])?;
        writeln!(
            out,
            "{BOLD}index {old_abbrev}..{new_abbrev} {mode_str}{RESET}"
        )?;
        for line in diff_body.lines() {
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
        writeln!(out, "diff --git a/{} b/{}", paths[0], paths[1])?;
        writeln!(out, "index {old_abbrev}..{new_abbrev} {mode_str}")?;
        write!(out, "{diff_body}")?;
    }

    if args.exit_code || args.quiet {
        std::process::exit(1);
    }
    std::process::exit(1);
}

/// Diff two directories recursively with --no-index.
fn run_no_index_dirs(args: Args, dir_a: &Path, dir_b: &Path) -> Result<()> {
    use std::collections::BTreeSet;

    /// Leaf content for diff: symlink target as bytes, or regular file bytes. Directories are not
    /// leaves (caller only collects real dirs and symlink files).
    fn read_no_index_leaf(path: &Path) -> Option<Vec<u8>> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() {
            return std::fs::read_link(path)
                .ok()
                .map(|t| t.to_string_lossy().into_owned().into_bytes());
        }
        if meta.is_file() {
            return std::fs::read(path).ok();
        }
        None
    }

    fn collect_files(dir: &Path, prefix: &str, out: &mut BTreeSet<String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let ft = entry.file_type()?;
            // Do not follow symlinks: `c -> b` must be one leaf (t2080), not a walk into `b/`.
            if ft.is_symlink() {
                out.insert(rel);
            } else if ft.is_dir() {
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

    let repo_opt = Repository::discover(None).ok();
    let merged_attrs = if let Some(ref r) = repo_opt {
        match load_gitattributes_for_diff(r) {
            Ok(m) => m,
            Err(Error::InvalidRef(msg)) if msg.starts_with("bad --attr-source") => {
                eprintln!("fatal: bad --attr-source or GIT_ATTR_SOURCE");
                std::process::exit(128);
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        grit_lib::attributes::ParsedGitAttributes::default()
    };
    let diff_config = repo_opt
        .as_ref()
        .map(|r| {
            grit_lib::config::ConfigSet::load(Some(&r.git_dir), true)
                .unwrap_or_else(|_| grit_lib::config::ConfigSet::new())
        })
        .unwrap_or_default();
    let ignore_case_attrs = diff_config
        .get("core.ignorecase")
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1"));
    let diff_algo_ctx = DiffAlgoContext {
        attrs: Arc::new(merged_attrs),
        config: Arc::new(diff_config),
        ignore_case_attrs,
    };
    let diff_algo_cli = parse_cli_diff_algorithm_from_argv();
    let patch_abbrev = if args.full_index {
        40usize
    } else if let Some(n) = args.abbrev {
        n.max(4).min(40)
    } else {
        7
    };

    let all_files: BTreeSet<_> = files_a.iter().chain(files_b.iter()).cloned().collect();
    let mut has_diff = false;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let context_lines = args.unified.unwrap_or(3);
    let inter_hunk_context = args.inter_hunk_context.unwrap_or(0);
    let ws_mode = WhitespaceMode {
        ignore_all_space: args.ignore_all_space,
        ignore_space_change: args.ignore_space_change,
        ignore_space_at_eol: args.ignore_space_at_eol,
        ignore_blank_lines: args.ignore_blank_lines,
        ignore_cr_at_eol: args.ignore_cr_at_eol,
    };

    for rel in &all_files {
        let fa = dir_a.join(rel);
        let fb = dir_b.join(rel);
        let data_a = read_no_index_leaf(&fa);
        let data_b = read_no_index_leaf(&fb);

        match (&data_a, &data_b) {
            (Some(a), Some(b)) if a == b => continue,
            _ => {}
        }

        let text_a = data_a
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).to_string())
            .unwrap_or_default();
        let text_b = data_b
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).to_string())
            .unwrap_or_default();
        if ws_mode.any() && ws_mode.normalize(&text_a) == ws_mode.normalize(&text_b) {
            continue;
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

        let display_old: &str = if data_a.is_none() {
            "/dev/null"
        } else {
            rel.as_str()
        };
        let display_new: &str = if data_b.is_none() {
            "/dev/null"
        } else {
            rel.as_str()
        };
        writeln!(out, "diff --git a/{rel} b/{rel}")?;
        let mode_a = fa
            .symlink_metadata()
            .ok()
            .and_then(|m| m.file_type().is_symlink().then_some("120000"))
            .unwrap_or("100644");
        let mode_b = fb
            .symlink_metadata()
            .ok()
            .and_then(|m| m.file_type().is_symlink().then_some("120000"))
            .unwrap_or("100644");
        if data_a.is_none() {
            writeln!(out, "new file mode {mode_b}")?;
        } else if data_b.is_none() {
            writeln!(out, "deleted file mode {mode_a}")?;
        }
        let da = data_a.as_deref().unwrap_or(&[]);
        let db = data_b.as_deref().unwrap_or(&[]);
        let a = abbrev_oid_hex(da, patch_abbrev);
        let b = abbrev_oid_hex(db, patch_abbrev);
        if mode_a == mode_b {
            writeln!(out, "index {a}..{b} {mode_a}")?;
        } else {
            writeln!(out, "index {a}..{b}")?;
            writeln!(out, "old mode {mode_a}")?;
            writeln!(out, "new mode {mode_b}")?;
        }
        let algo = diff_algorithm_for_path(rel.as_str(), diff_algo_cli, &diff_algo_ctx);
        let func_matcher = matcher_for_path_parsed(
            diff_algo_ctx.config.as_ref(),
            &diff_algo_ctx.attrs.rules,
            &diff_algo_ctx.attrs.macros,
            rel.as_str(),
            diff_algo_ctx.ignore_case_attrs,
        )
        .unwrap_or(None);
        let patch = unified_diff_with_prefix_and_funcname_and_algorithm(
            &text_a,
            &text_b,
            display_old,
            display_new,
            context_lines,
            inter_hunk_context,
            "a/",
            "b/",
            func_matcher.as_ref(),
            algo,
        );
        write!(out, "{patch}")?;
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
///
/// `cwd` is used to resolve relative orderfile paths (matches `git diff -O`).
/// Apply an orderfile to sort diff entries (public for use by other commands like log).
pub fn apply_orderfile_entries(
    entries: Vec<DiffEntry>,
    order_path: &str,
    cwd: &Path,
) -> Result<Vec<DiffEntry>> {
    apply_orderfile(entries, order_path, cwd)
}

fn apply_orderfile(
    mut entries: Vec<DiffEntry>,
    order_path: &str,
    cwd: &Path,
) -> Result<Vec<DiffEntry>> {
    let patterns = read_orderfile_patterns(order_path, cwd)?;
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
        patterns.len()
    };
    entries.sort_by_key(|e| sort_key(e));
    Ok(entries)
}

fn read_orderfile_patterns(order_path: &str, cwd: &Path) -> Result<Vec<String>> {
    let path = Path::new(order_path);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let _meta = std::fs::metadata(&resolved).map_err(|e| {
        anyhow::Error::new(ExplicitExit {
            code: 128,
            message: format!("could not read orderfile {order_path}: {e}"),
        })
    })?;
    let mut f = std::fs::File::open(&resolved).map_err(|e| {
        anyhow::Error::new(ExplicitExit {
            code: 128,
            message: format!("could not read orderfile {order_path}: {e}"),
        })
    })?;
    let mut content = String::new();
    std::io::Read::read_to_string(&mut f, &mut content).map_err(|e| {
        anyhow::Error::new(ExplicitExit {
            code: 128,
            message: format!("could not read orderfile {order_path}: {e}"),
        })
    })?;
    Ok(content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect())
}

/// Reorder diff entries for `git diff` `--rotate-to` / `--skip-to` (changed paths only).
pub fn apply_rotate_skip_entries(
    mut entries: Vec<DiffEntry>,
    rotate_to: Option<&str>,
    skip_to: Option<&str>,
) -> Result<Vec<DiffEntry>> {
    let Some(needle) = rotate_to.or(skip_to) else {
        return Ok(entries);
    };
    let needle = needle.trim();
    if needle.is_empty() {
        return Ok(entries);
    }
    let idx = entries
        .iter()
        .position(|e| e.path() == needle)
        .ok_or_else(|| {
            anyhow::Error::new(ExplicitExit {
                code: 128,
                message: format!("fatal: No such path '{needle}' in the diff"),
            })
        })?;
    if rotate_to.is_some() {
        entries.rotate_left(idx);
    }
    if let Some(skip) = skip_to.filter(|s| !s.trim().is_empty()) {
        let pos = entries
            .iter()
            .position(|e| e.path() == skip)
            .ok_or_else(|| {
                anyhow::Error::new(ExplicitExit {
                    code: 128,
                    message: format!("fatal: No such path '{skip}' in the diff"),
                })
            })?;
        entries.drain(..pos);
    }
    Ok(entries)
}

/// `git log` rotate/skip: reorder using the **commit tree** path order (all blobs), then keep only
/// paths present in `entries` — matches Git's `diff --rotate-to` with history walks.
pub fn apply_rotate_skip_log_entries(
    odb: &Odb,
    commit_tree: &ObjectId,
    entries: Vec<DiffEntry>,
    rotate_to: Option<&str>,
    skip_to: Option<&str>,
) -> Result<Vec<DiffEntry>> {
    let tree_paths = grit_lib::merge_diff::all_blob_paths_in_tree_order(odb, commit_tree);
    apply_rotate_skip_ordered_paths(&tree_paths, entries, rotate_to, skip_to)
}

fn apply_rotate_skip_ordered_paths(
    tree_paths: &[String],
    entries: Vec<DiffEntry>,
    rotate_to: Option<&str>,
    skip_to: Option<&str>,
) -> Result<Vec<DiffEntry>> {
    let rotate = rotate_to.and_then(|s| {
        let t = s.trim();
        (!t.is_empty()).then_some(t)
    });
    let skip = skip_to.and_then(|s| {
        let t = s.trim();
        (!t.is_empty()).then_some(t)
    });
    if rotate.is_none() && skip.is_none() {
        return Ok(entries);
    }

    use std::collections::HashMap;
    let mut by_path: HashMap<String, DiffEntry> = HashMap::new();
    for e in entries {
        by_path.insert(e.path().to_string(), e);
    }

    // `git log --skip-to`: only list changed paths from the skip point onward (unmodified paths
    // in the tree-order suffix are omitted). `--rotate-to` still lists every changed file in order.
    if rotate.is_none() {
        let Some(skip_path) = skip else {
            return Ok(by_path.into_values().collect());
        };
        let idx = tree_paths
            .iter()
            .position(|p| p == skip_path)
            .ok_or_else(|| {
                anyhow::Error::new(ExplicitExit {
                    code: 128,
                    message: format!("fatal: No such path '{skip_path}' in the diff"),
                })
            })?;
        let mut out = Vec::new();
        for p in tree_paths.iter().skip(idx) {
            if let Some(e) = by_path.remove(p) {
                out.push(e);
            }
        }
        return Ok(out);
    }

    let needle = rotate.expect("rotate set when reaching this branch");
    let idx = tree_paths.iter().position(|p| p == needle).ok_or_else(|| {
        anyhow::Error::new(ExplicitExit {
            code: 128,
            message: format!("fatal: No such path '{needle}' in the diff"),
        })
    })?;
    let mut order: Vec<String> = tree_paths.to_vec();
    order.rotate_left(idx);
    if let Some(skip_path) = skip {
        let pos = order.iter().position(|p| p == skip_path).ok_or_else(|| {
            anyhow::Error::new(ExplicitExit {
                code: 128,
                message: format!("fatal: No such path '{skip_path}' in the diff"),
            })
        })?;
        order.drain(..pos);
    }
    let mut out = Vec::new();
    for p in order {
        if let Some(e) = by_path.remove(&p) {
            out.push(e);
        }
    }
    Ok(out)
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
/// Parse `-M` / `--find-renames` similarity: optional trailing `%`, capped at 100 (Git truncates).
fn parse_diff_rename_threshold(raw: &str) -> u32 {
    let t = raw.trim();
    let num = t.strip_suffix('%').unwrap_or(t);
    num.parse::<u32>().unwrap_or(50).min(100)
}

/// Apply `--diff-filter` (include uppercase status letters, exclude lowercase).
fn apply_diff_filter(entries: Vec<DiffEntry>, filter: &str) -> Vec<DiffEntry> {
    let mut include: Option<std::collections::HashSet<char>> = None;
    let mut exclude: std::collections::HashSet<char> = std::collections::HashSet::new();
    for c in filter.chars() {
        if c == '*' {
            continue;
        }
        if c.is_ascii_uppercase() {
            include.get_or_insert_with(Default::default).insert(c);
        } else if c.is_ascii_lowercase() {
            exclude.insert(c.to_ascii_uppercase());
        }
    }
    entries
        .into_iter()
        .filter(|e| {
            let ch = e.status.letter();
            if exclude.contains(&ch) {
                return false;
            }
            if let Some(ref inc) = include {
                return inc.contains(&ch);
            }
            true
        })
        .collect()
}

/// Whether to emit unified patch hunks for this `git diff` invocation.
///
/// Git uses last-flag-wins among `-s` / `--no-patch` (suppress) and `-p` / `--patch` / `-u` /
/// `-U` / `--unified` (show patch). Combined short flags (e.g. `-sp`) are expanded per character.
fn diff_emit_unified_patch_from_argv(argv: &[String]) -> bool {
    let Some(diff_pos) = argv.iter().position(|a| a == "diff") else {
        return true;
    };
    let mut emit = true;
    for arg in argv.iter().skip(diff_pos + 1) {
        if arg == "--" {
            break;
        }
        if arg == "-s" || arg == "--no-patch" {
            emit = false;
            continue;
        }
        if arg == "-p" || arg == "--patch" || arg == "-u" {
            emit = true;
            continue;
        }
        if arg == "--submodule" || arg.starts_with("--submodule=") {
            emit = true;
            continue;
        }
        if arg.starts_with("-U") || arg.starts_with("--unified") {
            emit = true;
            continue;
        }
        if arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 2 {
            const COMBINABLE: &[u8] = b"spuwqRb";
            let bytes = arg.as_bytes();
            let tail = &bytes[1..];
            if !tail.is_empty() && tail.iter().all(|b| COMBINABLE.contains(b)) {
                for &b in tail {
                    match b {
                        b's' => emit = false,
                        b'p' | b'u' => emit = true,
                        b'w' | b'b' | b'q' | b'R' => {}
                        _ => {}
                    }
                }
            }
        }
    }
    emit
}

fn resolve_dirstat_options(
    args: &Args,
    git_dir: &std::path::Path,
    cli_active: bool,
) -> Result<(Option<DirstatOptions>, Vec<String>)> {
    use grit_lib::config::ConfigSet;
    let config = ConfigSet::load(Some(git_dir), false).unwrap_or_else(|_| ConfigSet::new());
    let mut warnings = Vec::new();

    let mut opts = DirstatOptions::default();

    if let Some(ref cfg_val) = config.get("diff.dirstat") {
        let (o, w) = parse_dirstat_params_lenient(cfg_val);
        warnings.extend(w);
        opts = o;
    }

    if args.dirstat_cumulative_flag {
        parse_dirstat_apply_tokens("cumulative", &mut opts, true, &mut warnings)?;
    }

    if let Some(ref p) = args.dirstat_by_file {
        parse_dirstat_apply_tokens("files", &mut opts, true, &mut warnings)?;
        if !p.is_empty() {
            parse_dirstat_apply_tokens(p, &mut opts, true, &mut warnings)?;
        }
    }

    for param in &args.dirstat {
        if param.is_empty() {
            opts = DirstatOptions::default();
        } else {
            parse_dirstat_apply_tokens(param, &mut opts, true, &mut warnings)?;
        }
    }

    if !cli_active && opts.is_default_everything() && config.get("diff.dirstat").is_none() {
        return Ok((None, warnings));
    }

    Ok((Some(opts), warnings))
}

fn parse_dirstat_percentage_permille(s: &str) -> Option<u32> {
    let mut parts = s.splitn(2, '.');
    let whole = parts.next()?.parse::<u32>().ok()?;
    let mut permille = whole.saturating_mul(10);
    if let Some(rest) = parts.next() {
        let mut chars = rest.chars();
        let d = chars.next()?;
        if !d.is_ascii_digit() {
            return None;
        }
        permille = permille.saturating_add(d.to_digit(10)?);
        if chars.next().is_some_and(|c| c.is_ascii_digit()) {
            // Git ignores further fractional digits after the first
        }
        if chars.any(|c| !c.is_ascii_digit()) {
            return None;
        }
    }
    Some(permille)
}

fn dirstat_damage_for_entry(
    odb: &Odb,
    entry: &DiffEntry,
    work_tree: Option<&Path>,
    break_rewrites: bool,
    mode: DirstatMode,
) -> u64 {
    let path = entry.path();
    let old_raw = read_content_raw(odb, &entry.old_oid);
    let new_raw = read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, path);

    match mode {
        DirstatMode::Files => 1,
        DirstatMode::Lines => {
            if break_rewrites
                && entry.status == DiffStatus::Modified
                && !is_binary(&old_raw)
                && !is_binary(&new_raw)
                && should_break_rewrite_for_stat(&old_raw, &new_raw)
            {
                let ins = count_git_lines(&new_raw) as u64;
                let del = count_git_lines(&old_raw) as u64;
                return ins.saturating_add(del);
            }
            let old_content = String::from_utf8_lossy(&old_raw).into_owned();
            let new_content = String::from_utf8_lossy(&new_raw).into_owned();
            let (ins, del) = count_changes(&old_content, &new_content);
            let mut damage = (ins as u64).saturating_add(del as u64);
            if is_binary(&old_raw) || is_binary(&new_raw) {
                damage = damage.div_ceil(64);
            }
            damage
        }
        DirstatMode::Changes => {
            if entry.old_oid == entry.new_oid {
                return 0;
            }
            if entry.status == DiffStatus::Added {
                return new_raw.len() as u64;
            }
            if entry.status == DiffStatus::Deleted {
                return old_raw.len() as u64;
            }
            let (copied, added) = diffcore_count_changes(&old_raw, &new_raw);
            let old_len = old_raw.len() as u64;
            let removed = old_len.saturating_sub(copied);
            let mut damage = removed.saturating_add(added);
            if damage == 0 {
                damage = 1;
            }
            damage
        }
    }
}

fn gather_dirstat_recursive(
    out: &mut impl Write,
    files: &[DirstatFile],
    idx: &mut usize,
    changed_total: u64,
    base_len: usize,
    base: &str,
    permille: u32,
    cumulative: bool,
) -> Result<u64> {
    let mut sum = 0u64;
    let mut sources = 0u32;

    while *idx < files.len() {
        let f = &files[*idx];
        let name = f.name.as_str();
        if name.len() < base_len {
            break;
        }
        if !name.starts_with(base) {
            break;
        }
        let rel = &name[base_len..];
        if let Some(slash_rel) = rel.find('/') {
            let slash_abs = base_len + slash_rel;
            let new_base_len = slash_abs + 1;
            let sub = gather_dirstat_recursive(
                out,
                files,
                idx,
                changed_total,
                new_base_len,
                &name[..new_base_len],
                permille,
                cumulative,
            )?;
            sum = sum.saturating_add(sub);
            sources = sources.saturating_add(1);
        } else {
            sum = sum.saturating_add(f.changed);
            *idx += 1;
            sources = sources.saturating_add(2);
        }
    }

    if base_len > 0 && sources != 1 && sum > 0 && changed_total > 0 {
        let permille_val = ((sum as u128) * 1000 / (changed_total as u128)) as u32;
        if permille_val >= permille {
            let int_part = permille_val / 10;
            let frac = permille_val % 10;
            writeln!(out, " {:>4}.{}% {}", int_part, frac, &base[..base_len])?;
            if !cumulative {
                return Ok(sum);
            }
        }
    }
    Ok(if cumulative { 0 } else { sum })
}

fn write_dirstat(
    out: &mut impl Write,
    opts: &DirstatOptions,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
    break_rewrites: bool,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut files: Vec<DirstatFile> = Vec::with_capacity(entries.len());
    let mut changed_total = 0u64;
    for e in entries {
        let name = e.path().to_string();
        let damage = dirstat_damage_for_entry(odb, e, work_tree, break_rewrites, opts.mode);
        changed_total = changed_total.saturating_add(damage);
        files.push(DirstatFile {
            name,
            changed: damage,
        });
    }
    if changed_total == 0 {
        return Ok(());
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let mut idx = 0usize;
    gather_dirstat_recursive(
        out,
        &files,
        &mut idx,
        changed_total,
        0,
        "",
        opts.permille,
        opts.cumulative,
    )?;
    Ok(())
}

/// When `core.precomposeunicode` is on, the work tree may store paths in NFC while argv uses NFD.
fn resolve_pathspec_for_diff_classification(arg: &str, precompose_unicode: bool) -> Option<String> {
    use std::path::Path;
    if Path::new(arg).symlink_metadata().is_ok() {
        return Some(arg.to_owned());
    }
    // With precompose, NFD argv must match NFC on-disk paths (macOS aliases them; Linux tests
    // force the config without FS aliasing, so the NFD spelling may not exist as a path).
    if precompose_unicode {
        let nfc = grit_lib::unicode_normalization::precompose_utf8_path(arg);
        if nfc.as_ref() != arg {
            return Some(nfc.into_owned());
        }
    }
    None
}

/// Prefix for `--relative` / `diff.relative` path stripping (trailing `/` except empty root).
///
/// `--no-relative` disables only the `diff.relative` config; explicit `--relative` on the CLI
/// still applies (matches Git, t4045).
fn resolve_diff_relative_prefix(
    work_tree: Option<&Path>,
    git_dir: &Path,
    args: &Args,
) -> Option<String> {
    let from_cli = match &args.relative {
        Some(Some(p)) if !p.is_empty() => Some(p.clone()),
        Some(_) => {
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
        None => None,
    };
    if from_cli.is_some() {
        return from_cli;
    }
    if args.no_relative {
        return None;
    }
    let config = ConfigSet::load(Some(git_dir), false).unwrap_or_else(|_| ConfigSet::new());
    match config.get("diff.relative") {
        Some(val) if matches!(val.to_lowercase().as_str(), "true" | "yes" | "1") => {
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

/// Map a `--relative`-stripped display path back to the repository-relative path for index I/O.
fn repo_relative_path_for_relative_display(display: &str, prefix: Option<&str>) -> String {
    let Some(pfx) = prefix.filter(|s| !s.is_empty()) else {
        return display.to_owned();
    };
    if pfx.ends_with('/') {
        format!("{pfx}{display}")
    } else {
        format!("{pfx}/{display}")
    }
}

/// Repository-relative path for attributes / worktree reads when `--relative` stripped display paths.
fn repo_path_for_diff_entry(entry: &DiffEntry, relative_prefix: Option<&str>) -> String {
    match relative_prefix {
        Some(p) if !p.is_empty() => repo_relative_path_for_relative_display(entry.path(), Some(p)),
        _ => entry.path().to_owned(),
    }
}

/// Repo-relative path for one diff side label (`old_path` / `new_path` are display paths after `--relative`).
fn repo_path_for_diff_side(display_path: &str, relative_prefix: Option<&str>) -> String {
    if display_path == "/dev/null" {
        return display_path.to_owned();
    }
    match relative_prefix {
        Some(p) if !p.is_empty() => repo_relative_path_for_relative_display(display_path, Some(p)),
        _ => display_path.to_owned(),
    }
}

fn parse_rev_and_paths(
    args: &[String],
    has_separator: bool,
    precompose_unicode: bool,
) -> (Vec<String>, Vec<String>) {
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
            } else if arg.starts_with(":!") || arg.starts_with(":^") {
                // Git pathspec exclusion (`:!` / `:^`); never a revision (t7012, `git diff HEAD :!path`).
                in_paths = true;
                paths.push(arg.clone());
            } else if let Some(p) =
                resolve_pathspec_for_diff_classification(arg, precompose_unicode)
            {
                in_paths = true;
                paths.push(p);
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

/// When `spec` is `treeish:path`, resolve the blob and path; otherwise `None`.
fn blob_side_for_treeish_path_spec(
    repo: &Repository,
    spec: &str,
) -> Result<Option<TreeishBlobAtPath>> {
    match split_treeish_colon(spec) {
        Some((_, path)) if !path.is_empty() => Ok(Some(resolve_treeish_blob_at_path(repo, spec)?)),
        _ => Ok(None),
    }
}

/// Resolve `spec` to a blob side for `git diff` when comparing two blobs (raw OID or `rev:path`).
fn blob_side_for_blob_diff_spec(
    repo: &Repository,
    spec: &str,
) -> Result<Option<TreeishBlobAtPath>> {
    if let Some(side) = blob_side_for_treeish_path_spec(repo, spec)? {
        return Ok(Some(side));
    }
    let oid = match resolve_revision(repo, spec) {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };
    let obj = match repo.odb.read(&oid) {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };
    if obj.kind != ObjectKind::Blob {
        return Ok(None);
    }
    Ok(Some(TreeishBlobAtPath {
        path: oid.to_hex(),
        oid,
        mode: "100644".to_owned(),
    }))
}

/// Octal mode string for a worktree file (`120000` symlink, `100755` executable, else `100644`).
fn worktree_file_mode_octal(path: &Path) -> String {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return "120000".to_owned();
        }
        #[cfg(unix)]
        {
            let mode = meta.permissions().mode();
            if mode & 0o111 != 0 {
                return "100755".to_owned();
            }
        }
    }
    "100644".to_owned()
}

/// If both sides are `rev:path` with `..` between path segments, return the two specs (Git: t4063).
fn try_treeish_blob_range(spec: &str) -> Option<(String, String)> {
    let bytes = spec.as_bytes();
    let mut i = 0usize;
    let mut peel_depth = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'^' && bytes[i + 1] == b'{' {
            peel_depth += 1;
            i += 2;
            continue;
        }
        if peel_depth > 0 {
            if bytes[i] == b'}' {
                peel_depth -= 1;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'.' && bytes.get(i + 1) == Some(&b'.') {
            let left = &spec[..i];
            let right = &spec[i + 2..];
            if split_treeish_colon(left).is_some_and(|(_, p)| !p.is_empty())
                && split_treeish_colon(right).is_some_and(|(_, p)| !p.is_empty())
            {
                return Some((left.to_owned(), right.to_owned()));
            }
        }
        i += 1;
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
///
/// Supports Git exclude pathspecs (`:!` / `:^`): when only exclusions are given, the
/// include set defaults to `.` (all paths), then exclusions are removed (same as `git rm`).
fn filter_by_paths(entries: Vec<DiffEntry>, paths: &[String]) -> Vec<DiffEntry> {
    if paths.is_empty() {
        return entries;
    }
    let mut include_specs: Vec<&str> = Vec::new();
    let mut exclude_inners: Vec<&str> = Vec::new();
    for spec in paths {
        if let Some(inner) = spec.strip_prefix(":!").or_else(|| spec.strip_prefix(":^")) {
            exclude_inners.push(inner);
        } else {
            include_specs.push(spec.as_str());
        }
    }
    if include_specs.is_empty() && !exclude_inners.is_empty() {
        include_specs.push(".");
    }
    entries
        .into_iter()
        .filter(|e| {
            let path = e.path();
            let included = include_specs.iter().any(|spec| {
                if spec == &"." || spec.is_empty() {
                    return true;
                }
                crate::pathspec::pathspec_matches(spec, path)
            });
            let excluded = exclude_inners
                .iter()
                .any(|inner| crate::pathspec::pathspec_matches(inner, path));
            included && !excluded
        })
        .collect()
}

/// Read content for a diff entry side, falling back to the working tree if
/// the OID is not in the ODB (worktree files are hashed but not stored).
fn read_content(odb: &Odb, oid: &ObjectId, work_tree: Option<&Path>, path: &str) -> String {
    let raw = read_content_raw_or_worktree(odb, oid, work_tree, path);
    String::from_utf8_lossy(&raw).into_owned()
}

/// Drop lines matching any of the `-I` / `--ignore-matching-lines` regexes (Git: ignore whole lines).
fn apply_ignore_matching_lines_to_text(text: &str, ignore: &[Regex]) -> String {
    if ignore.is_empty() {
        return text.to_owned();
    }
    let ends_with_nl = text.ends_with('\n');
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !ignore.iter().any(|re| re.is_match(line)))
        .collect();
    let mut out = kept.join("\n");
    if ends_with_nl && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// True when `-I` / `--ignore-matching-lines` removes all substantive differences (entry hidden like Git).
fn entry_hidden_by_line_ignore(
    entry: &DiffEntry,
    odb: &Odb,
    work_tree: Option<&Path>,
    ws_mode: &WhitespaceMode,
    ignore: &[Regex],
) -> bool {
    if ignore.is_empty() {
        return false;
    }
    if matches!(
        entry.status,
        DiffStatus::Renamed | DiffStatus::Copied | DiffStatus::TypeChanged | DiffStatus::Unmerged
    ) {
        return false;
    }
    if entry.old_mode != entry.new_mode {
        return false;
    }
    let old_path = entry.old_path.as_deref().unwrap_or(entry.path());
    let new_path = entry.new_path.as_deref().unwrap_or(entry.path());
    let (old, new) = match entry.status {
        DiffStatus::Added => (
            String::new(),
            read_content(odb, &entry.new_oid, work_tree, new_path),
        ),
        DiffStatus::Deleted => (
            read_content(odb, &entry.old_oid, work_tree, old_path),
            String::new(),
        ),
        _ => (
            read_content(odb, &entry.old_oid, work_tree, old_path),
            read_content(odb, &entry.new_oid, work_tree, new_path),
        ),
    };
    let mut old_f = apply_ignore_matching_lines_to_text(&old, ignore);
    let mut new_f = apply_ignore_matching_lines_to_text(&new, ignore);
    if ws_mode.any() {
        old_f = ws_mode.normalize(&old_f);
        new_f = ws_mode.normalize(&new_f);
    }
    old_f == new_f
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
        // Empty tree / new file side: read from the work tree when available (t1501 tree diffs).
        if let Some(wt) = work_tree {
            if path != "/dev/null" {
                if let Ok(data) = std::fs::read(wt.join(path)) {
                    return data;
                }
            }
        }
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

/// Insertion/deletion counts for `--stat` / `--shortstat` / `--numstat`.
///
/// When `--break-rewrites` is set and Git would treat the pair as a complete rewrite,
/// counts match Git's diffstat path (full line counts) instead of Myers line diff.
fn stat_ins_del_for_entry(
    odb: &Odb,
    entry: &DiffEntry,
    work_tree: Option<&Path>,
    break_rewrites: bool,
    line_ignore: Option<&[Regex]>,
    ws_mode: &WhitespaceMode,
    algo_ctx: &DiffAlgoContext,
    algo_cli: Option<similar::Algorithm>,
) -> (usize, usize) {
    let old_raw = read_content_raw(odb, &entry.old_oid);
    let new_raw = read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, entry.path());
    if break_rewrites
        && entry.status == DiffStatus::Modified
        && !is_binary(&old_raw)
        && !is_binary(&new_raw)
        && should_break_rewrite_for_stat(&old_raw, &new_raw)
    {
        return (count_git_lines(&new_raw), count_git_lines(&old_raw));
    }
    let mut old_content = String::from_utf8_lossy(&old_raw).into_owned();
    let mut new_content = String::from_utf8_lossy(&new_raw).into_owned();
    if let Some(ign) = line_ignore {
        if !ign.is_empty() {
            old_content = apply_ignore_matching_lines_to_text(&old_content, ign);
            new_content = apply_ignore_matching_lines_to_text(&new_content, ign);
        }
    }
    if ws_mode.any() {
        old_content = ws_mode.normalize(&old_content);
        new_content = ws_mode.normalize(&new_content);
    }
    let algo = diff_algorithm_for_path(entry.path(), algo_cli, algo_ctx);
    count_changes_with_algorithm(&old_content, &new_content, algo)
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
            let new_for_index_line =
                if entry.old_oid == zero_oid() && entry.new_oid == empty_blob_oid() {
                    empty_blob_oid()
                } else {
                    entry.new_oid
                };
            writeln!(
                out,
                "{b}index {}..{}{r}",
                abbr(&entry.old_oid),
                abbr(&new_for_index_line)
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
            if let Some(pct) = entry.score {
                writeln!(out, "{b}dissimilarity index {pct}%{r}")?;
            }
            // Pure mode change with identical blob: Git omits the `index` line (t3419-rebase-patch-id).
            if entry.old_oid == entry.new_oid && entry.old_mode != entry.new_mode {
                return Ok(());
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

/// Run `GIT_EXTERNAL_DIFF` / `diff.external` when set (Git-compatible argv: path, file, hex, mode ×2).
///
/// Matches Git's `prepare_shell_cmd` + `run_command` with `use_shell=1`.
fn run_external_diff_for_patch(
    out: &mut impl Write,
    cmd_line: &str,
    display_path: &str,
    old_raw: &[u8],
    new_raw: &[u8],
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    old_mode: &str,
    new_mode: &str,
) -> Result<()> {
    let cmd_line = cmd_line.trim();
    if cmd_line.is_empty() {
        bail!("empty external diff command");
    }
    let old_tmp = tempfile::NamedTempFile::new().context("temp file for external diff (old)")?;
    let new_tmp = tempfile::NamedTempFile::new().context("temp file for external diff (new)")?;
    fs::write(old_tmp.path(), old_raw)?;
    fs::write(new_tmp.path(), new_raw)?;
    let old_hex = old_oid.to_hex();
    let new_hex = new_oid.to_hex();
    const SHELL_META: &[char] = &[
        '|', '&', ';', '<', '>', '(', ')', '$', '`', '\\', '"', '\'', ' ', '\t', '\n', '*', '?',
        '[', '#', '~', '=', '%',
    ];
    let needs_c = cmd_line.chars().any(|c| SHELL_META.contains(&c));
    let mut cmd = Command::new("sh");
    if needs_c {
        let c_script = format!("{cmd_line} \"$@\"");
        cmd.arg("-c")
            .arg(&c_script)
            .arg(cmd_line)
            .arg(display_path)
            .arg(old_tmp.path())
            .arg(&old_hex)
            .arg(old_mode)
            .arg(new_tmp.path())
            .arg(&new_hex)
            .arg(new_mode);
    } else {
        cmd.arg(cmd_line)
            .arg(display_path)
            .arg(old_tmp.path())
            .arg(&old_hex)
            .arg(old_mode)
            .arg(new_tmp.path())
            .arg(&new_hex)
            .arg(new_mode);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn external diff {cmd_line:?}"))?;
    let mut stdout = child.stdout.take().context("external diff stdout")?;
    io::copy(&mut stdout, out)?;
    let status = child.wait().context("waiting for external diff")?;
    if !status.success() {
        bail!("external diff exited with {status}");
    }
    Ok(())
}

fn write_patch_with_prefix(
    out: &mut impl Write,
    repo: &Repository,
    entries: &[DiffEntry],
    odb: &Odb,
    git_dir: &Path,
    config: &grit_lib::config::ConfigSet,
    context_lines: usize,
    use_color: bool,
    word_diff: bool,
    work_tree: Option<&Path>,
    suppress_blank_empty: bool,
    abbrev_len: usize,
    inter_hunk_context: usize,
    show_binary: bool,
    break_rewrites: bool,
    irreversible_delete: bool,
    use_textconv: bool,
    src_prefix: &str,
    dst_prefix: &str,
    submodule_fmt: Option<&str>,
    submodule_ignore: SubmoduleIgnoreFlags,
    line_ignore: Option<&[Regex]>,
    algo_ctx: &DiffAlgoContext,
    algo_cli: Option<similar::Algorithm>,
    cached: bool,
    no_ext_diff: bool,
    external_diff_cmd: Option<&str>,
    relative_prefix: Option<&str>,
) -> Result<()> {
    for entry in entries {
        let old_path = entry.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = entry.new_path.as_deref().unwrap_or("/dev/null");
        let path_for_attrs = repo_path_for_diff_entry(entry, relative_prefix);

        if let Some(fmt) = submodule_fmt {
            if entry.old_mode == "160000" || entry.new_mode == "160000" {
                if fmt == "log"
                    && entry.old_mode == "160000"
                    && entry.new_mode == "160000"
                    && matches!(
                        entry.status,
                        DiffStatus::Modified | DiffStatus::Renamed | DiffStatus::Copied
                    )
                    && entry.old_oid != entry.new_oid
                {
                    write_submodule_log_lines(out, repo, entry)?;
                    continue;
                }
                if fmt == "diff" {
                    write_patch_entry(
                        out,
                        repo,
                        odb,
                        entry,
                        context_lines,
                        work_tree,
                        true,
                        submodule_ignore,
                        &path_for_attrs,
                    )?;
                    continue;
                }
            }
        }

        let old_wt_path = repo_path_for_diff_side(old_path, relative_prefix);
        let old_content_raw = if entry.old_oid == zero_oid() {
            read_content_raw_or_worktree(odb, &entry.old_oid, work_tree, &old_wt_path)
        } else {
            read_content_raw(odb, &entry.old_oid)
        };
        let wt_path = path_for_attrs.clone();
        let new_content_raw =
            read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, &wt_path);

        if !no_ext_diff {
            if let Some(ext) = external_diff_cmd.filter(|s| !s.is_empty()) {
                if entry.status != DiffStatus::Unmerged {
                    let display = entry.path();
                    run_external_diff_for_patch(
                        out,
                        ext,
                        display,
                        &old_content_raw,
                        &new_content_raw,
                        &entry.old_oid,
                        &entry.new_oid,
                        &entry.old_mode,
                        &entry.new_mode,
                    )?;
                    continue;
                }
            }
        }

        write_diff_header_with_prefix(out, entry, use_color, abbrev_len, src_prefix, dst_prefix)?;

        // Submodule gitlink (mode 160000): emit `Subproject commit` hunks like Git — the ODB
        // does not store these as blobs in the superproject (`git apply` / t4137 rely on this).
        if entry.old_mode == "160000" || entry.new_mode == "160000" {
            let (old_label, new_label) = match entry.status {
                DiffStatus::Added => ("/dev/null".to_owned(), format!("{dst_prefix}{new_path}")),
                DiffStatus::Deleted => (format!("{src_prefix}{old_path}"), "/dev/null".to_owned()),
                _ => (
                    format!("{src_prefix}{old_path}"),
                    format!("{dst_prefix}{new_path}"),
                ),
            };
            writeln!(out, "--- {old_label}")?;
            writeln!(out, "+++ {new_label}")?;
            match entry.status {
                DiffStatus::Added => {
                    writeln!(out, "@@ -0,0 +1 @@")?;
                    writeln!(out, "+Subproject commit {}", entry.new_oid.to_hex())?;
                }
                DiffStatus::Deleted => {
                    writeln!(out, "@@ -1 +0,0 @@")?;
                    writeln!(out, "-Subproject commit {}", entry.old_oid.to_hex())?;
                }
                DiffStatus::Modified | DiffStatus::Renamed | DiffStatus::Copied => {
                    if entry.old_mode == "160000" && entry.new_mode == "160000" {
                        writeln!(out, "@@ -1 +1 @@")?;
                        writeln!(out, "-Subproject commit {}", entry.old_oid.to_hex())?;
                        writeln!(out, "+Subproject commit {}", entry.new_oid.to_hex())?;
                    } else if entry.old_mode == "160000" {
                        writeln!(out, "@@ -1 +0,0 @@")?;
                        writeln!(out, "-Subproject commit {}", entry.old_oid.to_hex())?;
                    } else {
                        writeln!(out, "@@ -0,0 +1 @@")?;
                        writeln!(out, "+Subproject commit {}", entry.new_oid.to_hex())?;
                    }
                }
                DiffStatus::TypeChanged => {
                    if entry.old_mode == "160000" {
                        writeln!(out, "@@ -1 +0,0 @@")?;
                        writeln!(out, "-Subproject commit {}", entry.old_oid.to_hex())?;
                    } else if entry.new_mode == "160000" {
                        writeln!(out, "@@ -0,0 +1 @@")?;
                        writeln!(out, "+Subproject commit {}", entry.new_oid.to_hex())?;
                    }
                }
                DiffStatus::Unmerged => {}
            }
            continue;
        }

        // Check for binary content
        // `git diff --cached` for a newly staged empty file: header + index line only, no hunks
        // (t1501-work-tree `diff-TREE-cached.expected`).
        if cached && entry.status == DiffStatus::Added && new_content_raw.is_empty() {
            continue;
        }

        if entry.status == DiffStatus::Modified
            && entry.old_oid == entry.new_oid
            && entry.old_mode != entry.new_mode
        {
            continue;
        }

        if entry.status == DiffStatus::Deleted && irreversible_delete {
            continue;
        }

        let textconv_patch =
            use_textconv && diff_textconv_active(git_dir, config, path_for_attrs.as_str());
        if !textconv_patch && (is_binary(&old_content_raw) || is_binary(&new_content_raw)) {
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

        let mut old_content = if entry.old_oid == zero_oid() {
            String::new()
        } else if use_textconv {
            blob_text_for_diff_with_oid(
                odb,
                git_dir,
                config,
                path_for_attrs.as_str(),
                &old_content_raw,
                &entry.old_oid,
                true,
            )
        } else {
            String::from_utf8_lossy(&old_content_raw).into_owned()
        };
        let mut new_content = if entry.new_oid == zero_oid() {
            String::new()
        } else if use_textconv {
            blob_text_for_diff_with_oid(
                odb,
                git_dir,
                config,
                path_for_attrs.as_str(),
                &new_content_raw,
                &entry.new_oid,
                true,
            )
        } else {
            String::from_utf8_lossy(&new_content_raw).into_owned()
        };
        if let Some(ign) = line_ignore {
            if !ign.is_empty() {
                old_content = apply_ignore_matching_lines_to_text(&old_content, ign);
                new_content = apply_ignore_matching_lines_to_text(&new_content, ign);
            }
        }

        // Intent-to-add empty file: header + `index 0000000..e69de29` only (t2203).
        if !cached
            && entry.status == DiffStatus::Added
            && entry.old_oid == zero_oid()
            && old_content.is_empty()
            && new_content.is_empty()
        {
            continue;
        }

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

        if break_rewrites
            && irreversible_delete
            && entry.status == DiffStatus::Modified
            && entry.score.is_some()
        {
            let new_lc = count_git_lines(&new_content_raw);
            let new_start = if new_lc == 0 { 0 } else { 1 };
            writeln!(
                out,
                "--- {}",
                if display_old == "/dev/null" {
                    "/dev/null".to_owned()
                } else {
                    format!("{src_prefix}{display_old}")
                }
            )?;
            writeln!(
                out,
                "+++ {}",
                if display_new == "/dev/null" {
                    "/dev/null".to_owned()
                } else {
                    format!("{dst_prefix}{display_new}")
                }
            )?;
            writeln!(out, "@@ -?,? +{new_start},{new_lc} @@")?;
            for chunk in new_content.split_inclusive('\n') {
                if chunk.ends_with('\n') {
                    let body = chunk.strip_suffix('\n').unwrap_or(chunk);
                    writeln!(out, "+{body}")?;
                } else if !chunk.is_empty() {
                    writeln!(out, "+{chunk}")?;
                    writeln!(out, "\\ No newline at end of file")?;
                }
            }
            continue;
        }

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
            let algo = diff_algorithm_for_path(path_for_attrs.as_str(), algo_cli, algo_ctx);
            let func_matcher = matcher_for_path_parsed(
                algo_ctx.config.as_ref(),
                &algo_ctx.attrs.rules,
                &algo_ctx.attrs.macros,
                path_for_attrs.as_str(),
                algo_ctx.ignore_case_attrs,
            )
            .unwrap_or(None);
            let patch = unified_diff_with_prefix_and_funcname_and_algorithm(
                &old_content,
                &new_content,
                display_old,
                display_new,
                context_lines,
                inter_hunk_context,
                src_prefix,
                dst_prefix,
                func_matcher.as_ref(),
                algo,
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
    break_rewrites: bool,
    line_ignore: Option<&[Regex]>,
    ws_mode: &WhitespaceMode,
    algo_ctx: &DiffAlgoContext,
    algo_cli: Option<similar::Algorithm>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let mut total_ins = 0usize;
    let mut total_del = 0usize;
    let mut files_changed = 0usize;

    for entry in entries {
        let (ins, del) = stat_ins_del_for_entry(
            odb,
            entry,
            work_tree,
            break_rewrites,
            line_ignore,
            ws_mode,
            algo_ctx,
            algo_cli,
        );
        total_ins += ins;
        total_del += del;
        files_changed += 1;
    }

    if files_changed == 0 {
        return Ok(());
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

/// Write a stat summary for each entry, followed by a totals line.
fn write_stat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
    stat_count: Option<usize>,
    stat_width: Option<usize>,
    stat_name_width: Option<usize>,
    break_rewrites: bool,
    stat_graph_width: Option<usize>,
    line_prefix: &str,
    git_dir: &Path,
    line_ignore: Option<&[Regex]>,
    ws_mode: &WhitespaceMode,
    quote_path_fully: bool,
    algo_ctx: &DiffAlgoContext,
    algo_cli: Option<similar::Algorithm>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let display_paths: Vec<String> = entries
        .iter()
        .map(|e| match e.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let old = e.old_path.as_deref().unwrap_or("");
                let new = e.new_path.as_deref().unwrap_or("");
                format_rename_display(old, new, quote_path_fully)
            }
            _ => grit_lib::quote_path::quote_c_style(e.path(), quote_path_fully),
        })
        .collect();

    let mut files: Vec<FileStatInput> = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        if entry.status == DiffStatus::Unmerged {
            continue;
        }
        let old_raw = read_content_raw(odb, &entry.old_oid);
        let new_raw = read_content_raw_or_worktree(odb, &entry.new_oid, work_tree, entry.path());
        let binary = is_binary(&old_raw) || is_binary(&new_raw);
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
                path_display: display_paths[i].clone(),
                insertions: added,
                deletions: deleted,
                is_binary: true,
            });
        } else {
            let mode_only = entry.status == DiffStatus::Modified
                && entry.old_mode != entry.new_mode
                && old_raw == new_raw;
            let (ins, del) = if mode_only {
                (0, 0)
            } else {
                stat_ins_del_for_entry(
                    odb,
                    entry,
                    work_tree,
                    break_rewrites,
                    line_ignore,
                    ws_mode,
                    algo_ctx,
                    algo_cli,
                )
            };
            files.push(FileStatInput {
                path_display: display_paths[i].clone(),
                insertions: ins,
                deletions: del,
                is_binary: false,
            });
        }
    }

    let total_w = stat_width.unwrap_or_else(terminal_columns);
    let cfg = grit_lib::config::ConfigSet::load(Some(git_dir), false)
        .unwrap_or_else(|_| grit_lib::config::ConfigSet::new());
    let eff_name = stat_name_width.or_else(|| {
        cfg.get("diff.statNameWidth")
            .and_then(|s| s.parse::<usize>().ok())
    });
    let eff_graph = stat_graph_width.or_else(|| {
        cfg.get("diff.statGraphWidth")
            .and_then(|s| s.parse::<usize>().ok())
    });

    let opts = DiffstatOptions {
        total_width: total_w,
        line_prefix,
        subtract_prefix_from_terminal: stat_width.is_none() && !line_prefix.is_empty(),
        stat_name_width: eff_name,
        stat_graph_width: eff_graph,
        stat_count,
        color_add: "",
        color_del: "",
        color_reset: "",
        graph_bar_slack: 0,
        graph_prefix_budget_slack: 0,
    };
    write_diffstat_block(out, &files, &opts)?;
    Ok(())
}

/// Format a rename/copy path for numstat: `{old_quoted}\t{new_quoted}` or
/// `{old_quoted} => {new_quoted}` depending on format.
fn format_rename_display(old: &str, new: &str, quote_path_fully: bool) -> String {
    // Use the pretty-print format with common prefix/suffix like c/{b/a => d/e}
    let pretty = grit_lib::diff::format_rename_path(old, new);
    grit_lib::quote_path::quote_c_style(&pretty, quote_path_fully)
}

/// Write machine-readable numstat output: `{insertions}\t{deletions}\t{path}`.
fn write_numstat(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
    break_rewrites: bool,
    line_ignore: Option<&[Regex]>,
    ws_mode: &WhitespaceMode,
    algo_ctx: &DiffAlgoContext,
    algo_cli: Option<similar::Algorithm>,
) -> Result<()> {
    for entry in entries {
        let (ins, del) = stat_ins_del_for_entry(
            odb,
            entry,
            work_tree,
            break_rewrites,
            line_ignore,
            ws_mode,
            algo_ctx,
            algo_cli,
        );
        match entry.status {
            DiffStatus::Renamed | DiffStatus::Copied => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let display = format_rename_display(old, new, false);
                writeln!(out, "{ins}\t{del}\t{display}")?;
            }
            _ => {
                writeln!(out, "{ins}\t{del}\t{}", entry.path())?;
            }
        }
    }
    Ok(())
}

/// Write only the names of changed files.
/// Write `--summary` output for rename/copy/mode-change entries.
fn write_diff_summary(
    out: &mut impl Write,
    entries: &[DiffEntry],
    break_rewrites: bool,
    quote_path_fully: bool,
) -> Result<()> {
    for entry in entries {
        match entry.status {
            DiffStatus::Renamed => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let display = format_rename_display(old, new, quote_path_fully);
                let sim = entry.score.unwrap_or(100);
                writeln!(out, " rename {display} ({sim}%)")?;
            }
            DiffStatus::Copied => {
                let old = entry.old_path.as_deref().unwrap_or("");
                let new = entry.new_path.as_deref().unwrap_or("");
                let display = format_rename_display(old, new, quote_path_fully);
                let sim = entry.score.unwrap_or(100);
                writeln!(out, " copy {display} ({sim}%)")?;
            }
            DiffStatus::Added => {
                writeln!(
                    out,
                    " create mode {} {}",
                    entry.new_mode,
                    grit_lib::quote_path::quote_c_style(entry.path(), quote_path_fully)
                )?;
            }
            DiffStatus::Deleted => {
                writeln!(
                    out,
                    " delete mode {} {}",
                    entry.old_mode,
                    grit_lib::quote_path::quote_c_style(entry.path(), quote_path_fully)
                )?;
            }
            DiffStatus::Modified => {
                if break_rewrites {
                    if let Some(pct) = entry.score {
                        writeln!(
                            out,
                            " rewrite {} ({pct}%)",
                            grit_lib::quote_path::quote_c_style(entry.path(), quote_path_fully)
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_name_only(
    out: &mut impl Write,
    entries: &[DiffEntry],
    quote_path_fully: bool,
) -> Result<()> {
    for entry in entries {
        writeln!(
            out,
            "{}",
            grit_lib::quote_path::quote_c_style(entry.path(), quote_path_fully)
        )?;
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
            DiffStatus::Modified => {
                if let Some(pct) = entry.score {
                    writeln!(
                        out,
                        ":{old_mode} {new_mode} {old_oid} {new_oid} {status}{pct:03}\t{}",
                        entry.path()
                    )?;
                } else {
                    writeln!(
                        out,
                        ":{old_mode} {new_mode} {old_oid} {new_oid} {status}\t{}",
                        entry.path()
                    )?;
                }
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

fn write_name_status(
    out: &mut impl Write,
    entries: &[DiffEntry],
    quote_path_fully: bool,
) -> Result<()> {
    for entry in entries {
        match entry.status {
            DiffStatus::Renamed => {
                let s = entry.score.unwrap_or(100);
                writeln!(
                    out,
                    "R{:03}\t{}\t{}",
                    s,
                    grit_lib::quote_path::quote_c_style(
                        entry.old_path.as_deref().unwrap_or(""),
                        quote_path_fully,
                    ),
                    grit_lib::quote_path::quote_c_style(
                        entry.new_path.as_deref().unwrap_or(""),
                        quote_path_fully,
                    ),
                )?;
            }
            DiffStatus::Copied => {
                let s = entry.score.unwrap_or(100);
                writeln!(
                    out,
                    "C{:03}\t{}\t{}",
                    s,
                    grit_lib::quote_path::quote_c_style(
                        entry.old_path.as_deref().unwrap_or(""),
                        quote_path_fully,
                    ),
                    grit_lib::quote_path::quote_c_style(
                        entry.new_path.as_deref().unwrap_or(""),
                        quote_path_fully,
                    ),
                )?;
            }
            _ => {
                writeln!(
                    out,
                    "{}\t{}",
                    entry.status.letter(),
                    grit_lib::quote_path::quote_c_style(entry.path(), quote_path_fully)
                )?;
            }
        }
    }
    Ok(())
}

/// Git default conflict marker width (`DEFAULT_CONFLICT_MARKER_SIZE` in upstream).
const DEFAULT_CONFLICT_MARKER_SIZE: usize = 7;

fn conflict_marker_size_for_path(
    rules: Option<&[AttrRule]>,
    rel_path: &str,
    config: &ConfigSet,
) -> usize {
    let mut size = DEFAULT_CONFLICT_MARKER_SIZE;
    if let Some(rules) = rules {
        let fa = get_file_attrs(rules, rel_path, false, config);
        if let Some(ref s) = fa.conflict_marker_size {
            if let Ok(n) = s.trim().parse::<i32>() {
                if n > 0 {
                    size = n as usize;
                }
            }
        }
    }
    size
}

/// Match upstream `is_conflict_marker` in `git/diff.c`.
fn is_conflict_marker_line(body: &str, marker_size: usize) -> bool {
    let line = body.trim_end_matches(['\n', '\r']);
    let len = line.len();
    if len < marker_size {
        return false;
    }
    let first = match line.as_bytes().first().copied() {
        Some(b) => b,
        None => return false,
    };
    if !matches!(first, b'=' | b'>' | b'<' | b'|') {
        return false;
    }
    for i in 1..marker_size {
        if line.as_bytes().get(i).copied() != Some(first) {
            return false;
        }
    }
    // Middle conflict line is exactly `marker_size` '=' characters (no trailing label).
    if first == b'=' && len == marker_size {
        return true;
    }
    if len < marker_size + 1 {
        return false;
    }
    line.as_bytes()
        .get(marker_size)
        .is_some_and(|b| b.is_ascii_whitespace())
}

/// Check for whitespace errors in added/modified lines.
/// Returns true if any errors were found.
fn check_whitespace_errors(
    out: &mut impl Write,
    entries: &[DiffEntry],
    odb: &Odb,
    work_tree: Option<&Path>,
    attr_rules: Option<&[AttrRule]>,
    config: &ConfigSet,
) -> Result<bool> {
    use grit_lib::diff::zero_oid;
    let mut has_errors = false;

    for entry in entries {
        if entry.status == DiffStatus::Deleted {
            continue;
        }
        let path = entry.path();
        let marker_size = conflict_marker_size_for_path(attr_rules, path, config);

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
                    // Check for conflict markers
                    let bare = line.trim_end_matches('\n').trim_end_matches('\r');
                    if is_conflict_marker_line(bare, marker_size) {
                        writeln!(out, "{}:{}: leftover conflict marker", path, line_no)?;
                        write!(out, "+{}", line)?;
                        if !line.ends_with('\n') {
                            writeln!(out)?;
                        }
                        has_errors = true;
                    }
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
fn resolve_diff_prefixes(args: &Args, repo: &Repository, cached: bool) -> (String, String) {
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
                // Matches Git: cached diff is commit vs index (`c/` vs `i/`); otherwise index vs worktree (`i/` vs `w/`).
                if cached {
                    return ("c/".to_owned(), "i/".to_owned());
                }
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

/// Write unified diff output for a list of DiffEntry pairs.
pub fn write_patch_from_pairs(
    _out: &mut dyn std::io::Write,
    _entries: &[DiffEntry],
    _repo: &Repository,
) -> anyhow::Result<()> {
    // Stub: full implementation pending
    Ok(())
}
