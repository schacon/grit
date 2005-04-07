//! `grit log` — show commit logs.
//!
//! Displays the commit history starting from HEAD (or specified revisions),
//! with configurable formatting and filtering.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use grit_lib::diff::{
    count_changes, diff_trees, format_raw, format_stat_line, unified_diff, DiffEntry, DiffStatus,
};
use grit_lib::objects::{parse_commit, ObjectId};
use grit_lib::odb::Odb;
use grit_lib::reflog::read_reflog;
use grit_lib::refs::list_refs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{
    collect_revision_specs_with_stdin, rev_list, OrderingMode, RevListOptions,
};
use grit_lib::state::{resolve_head, HeadState};
use regex::{Regex, RegexBuilder};
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Kind of ref decoration (maps to Git `decoration_type` / `color.decorate.*` slots).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DecorationKind {
    Head,
    LocalBranch,
    Remote,
    Tag,
    Stash,
    Grafted,
}

/// A single ref attached to a commit for `--decorate` display.
#[derive(Clone, Debug)]
struct RefDecoration {
    kind: DecorationKind,
    /// Canonical ref name (e.g. `refs/heads/main`, `refs/tags/v1.0`).
    full_name: String,
}

/// Parsed `color.decorate.*` and `diff.color.commit` sequences for log decorations.
#[derive(Clone, Debug, Default)]
struct DecorationColorStyle {
    commit: String,
    reset: String,
    branch: String,
    remote: String,
    tag: String,
    stash: String,
    head: String,
    grafted: String,
}

impl DecorationColorStyle {
    fn slot(&self, kind: DecorationKind) -> &str {
        match kind {
            DecorationKind::Head => &self.head,
            DecorationKind::LocalBranch => &self.branch,
            DecorationKind::Remote => &self.remote,
            DecorationKind::Tag => &self.tag,
            DecorationKind::Stash => &self.stash,
            DecorationKind::Grafted => &self.grafted,
        }
    }
}

fn load_decoration_color_style(git_dir: &Path, use_color: bool) -> DecorationColorStyle {
    let mut s = DecorationColorStyle::default();
    if !use_color {
        return s;
    }
    let Ok(config) = grit_lib::config::ConfigSet::load(Some(git_dir), true) else {
        return s;
    };
    if let Some(v) = config.get("diff.color.commit") {
        s.commit = format_ansi_color_spec(v.as_str());
    }
    s.reset = "\x1b[m".to_owned();
    if let Some(v) = config.get("color.decorate.branch") {
        s.branch = format_ansi_color_spec(v.as_str());
    }
    if let Some(v) = config.get("color.decorate.remoteBranch") {
        s.remote = format_ansi_color_spec(v.as_str());
    }
    if let Some(v) = config.get("color.decorate.tag") {
        s.tag = format_ansi_color_spec(v.as_str());
    }
    if let Some(v) = config.get("color.decorate.stash") {
        s.stash = format_ansi_color_spec(v.as_str());
    }
    if let Some(v) = config.get("color.decorate.HEAD") {
        s.head = format_ansi_color_spec(v.as_str());
    }
    if let Some(v) = config.get("color.decorate.grafted") {
        s.grafted = format_ansi_color_spec(v.as_str());
    }
    s
}

fn replace_ref_base() -> String {
    std::env::var("GIT_REPLACE_REF_BASE")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| if v.ends_with('/') { v } else { format!("{v}/") })
        .unwrap_or_else(|| "refs/replace/".to_owned())
}

fn prettify_refname(full_name: &str) -> &str {
    full_name
        .strip_prefix("refs/heads/")
        .or_else(|| full_name.strip_prefix("refs/tags/"))
        .or_else(|| full_name.strip_prefix("refs/remotes/"))
        .unwrap_or(full_name)
}

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

    /// Skip this many commits.
    #[arg(long = "skip")]
    pub skip: Option<usize>,

    /// Filter by author (regex pattern).
    #[arg(long = "author")]
    pub author: Option<String>,

    /// Filter by committer (regex pattern).
    #[arg(long = "committer")]
    pub committer_filter: Option<String>,

    /// Filter by commit message (regex pattern).
    #[arg(long = "grep")]
    pub grep: Option<String>,

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

    /// Alias for --patch.
    #[arg(short = 'u', hide = true)]
    pub patch_u: bool,

    /// Show diffstat per commit.
    #[arg(long = "stat")]
    pub stat: bool,

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

    /// Follow file renames (single file only).
    #[arg(long = "follow")]
    pub follow: bool,

    /// Filter by change type (A=added, M=modified, D=deleted, R=renamed, C=copied).
    #[arg(long = "diff-filter")]
    pub diff_filter: Option<String>,

    /// Only show commits that add or remove the given object.
    #[arg(long = "find-object")]
    pub find_object: Option<String>,

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

    /// Produce dense combined diff for merge commits.
    #[arg(long = "cc")]
    pub cc: bool,

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

    /// Grep log messages.
    #[arg(long = "grep", value_name = "PATTERN")]
    pub grep_patterns: Vec<String>,

    /// Invert grep match.
    #[arg(long = "invert-grep")]
    pub invert_grep: bool,

    /// Case insensitive grep.
    #[arg(short = 'i', long = "regexp-ignore-case")]
    pub regexp_ignore_case: bool,

    /// All --grep patterns must match.
    #[arg(long = "all-match")]
    pub all_match: bool,

    /// Use basic regexp for --grep.
    #[arg(short = 'G', long = "basic-regexp")]
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

    /// Show full object hashes in diff output.
    #[arg(long = "full-index")]
    pub full_index: bool,

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

    /// Generate diff with N lines of context.
    #[arg(short = 'U', long = "unified", value_name = "N")]
    pub unified: Option<usize>,
}

/// Extract epoch timestamp from a Git ident string.
fn extract_epoch_from_ident(ident: &str) -> i64 {
    if let Some(gt) = ident.rfind('>') {
        let after = ident[gt + 1..].trim();
        if let Some(epoch_str) = after.split_whitespace().next() {
            return epoch_str.parse::<i64>().unwrap_or(0);
        }
    }
    0
}

/// Parse a date string into a Unix epoch timestamp.
fn parse_date_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
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

fn run_graph_log(
    repo: &Repository,
    args: &Args,
    decoration_colors: Option<&DecorationColorStyle>,
) -> Result<()> {
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
            match resolve_revision(repo, stripped) {
                Ok(_) => revision_specs.push(rev.clone()),
                Err(_err) if is_likely_pathspec_during_rev_parse(stripped) => {
                    implied_pathspecs.push(stripped.to_owned())
                }
                Err(err) => return Err(err),
            }
        } else {
            match resolve_revision(repo, rev) {
                Ok(_) => revision_specs.push(rev.clone()),
                Err(_err) if is_likely_pathspec_during_rev_parse(rev) => {
                    implied_pathspecs.push(rev.clone())
                }
                Err(err) => return Err(err),
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

    let mut result = rev_list(repo, &positive_specs, &negative_specs, &options)
        .map_err(|e| anyhow::anyhow!("rev-list failed: {e}"))?;

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
    let force_first_parent_for_graph =
        args.sparse && !combined_pathspecs.is_empty() && !args.full_history;
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
            args.first_parent || force_first_parent_for_graph,
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

    let decorations = if args.simplify_by_decoration {
        Some(collect_decorations(repo, false)?)
    } else {
        None
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut graph = AsciiGraph::new();
    let line_prefix = args.line_prefix.as_deref().unwrap_or("");
    let abbrev_len = effective_abbrev_len(args);

    for node in nodes {
        let info = load_commit_info(repo, node.oid)?;
        graph.update(node.clone());

        loop {
            let (line, shown_commit_line) = graph.next_line();
            if shown_commit_line {
                let rendered = render_graph_commit_text(
                    &node,
                    &info,
                    args,
                    decorations.as_ref(),
                    decoration_colors,
                    abbrev_len,
                );
                writeln!(out, "{line_prefix}{line}{rendered}")?;
                break;
            }
            writeln!(out, "{line_prefix}{line}")?;
        }

        while !graph.is_commit_finished() {
            let (line, _) = graph.next_line();
            writeln!(out, "{line_prefix}{line}")?;
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
    if first_parent_only && parents.len() > 1 {
        parents.truncate(1);
    } else if !first_parent_only {
        parents.truncate(1);
    }
    for parent in parents {
        collect_visible_parent_for_graph(repo, parent, included, first_parent_only, seen, out)?;
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
        let visible = visible_parents_for_graph(repo, oid, &included, first_parent_only, false)?;
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
    decorations: Option<&HashMap<String, Vec<RefDecoration>>>,
    decoration_colors: Option<&DecorationColorStyle>,
    abbrev_len: usize,
) -> String {
    let hex = node.oid.to_hex();
    if args.oneline || args.format.as_deref() == Some("oneline") {
        let first_line = info.message.lines().next().unwrap_or("");
        let dec = format_decoration(&hex, decorations, decoration_colors);
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
                decoration_colors,
                args.date.as_deref(),
                abbrev_len,
                false,
            );
        }
        if fmt.contains('%') {
            return apply_format_string(
                fmt,
                &node.oid,
                info,
                decorations,
                decoration_colors,
                args.date.as_deref(),
                abbrev_len,
                false,
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

        if line.len() < self.width {
            line.push_str(&" ".repeat(self.width - line.len()));
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

/// Run the `log` command.
pub fn run(mut args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    validate_pathspec_scope(&repo, &args.pathspecs)?;
    let mut implied_pathspecs: Vec<String> = Vec::new();

    // Determine color mode
    let use_color = if args.no_color {
        false
    } else if let Some(ref c) = args.color {
        c == "always" || c == "true" || c.is_empty()
    } else {
        // Check config for color.diff / color.ui
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
    };

    let decoration_colors_storage = if use_color {
        Some(load_decoration_color_style(&repo.git_dir, true))
    } else {
        None
    };
    let decoration_colors = decoration_colors_storage.as_ref();

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
        return run_reflog_walk(&repo, &args);
    }

    // Handle --no-walk: show given commits without walking parents
    if args.no_walk.is_some() {
        return run_no_walk(&repo, &args, decoration_colors);
    }

    if args.graph {
        return run_graph_log(&repo, &args, decoration_colors);
    }

    // Determine starting points and excluded commits.
    // Revisions prefixed with `^` (e.g. `^HEAD`) mean "exclude this and its
    // ancestors" — standard git revision range syntax.
    let (start_oids, exclude_oids) = if args.all {
        (collect_all_ref_oids(&repo.git_dir)?, Vec::new())
    } else if args.revisions.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        match head.oid() {
            Some(oid) => (vec![*oid], Vec::new()),
            None => {
                anyhow::bail!("your current branch 'main' does not have any commits yet");
            }
        }
    } else {
        let mut oids = Vec::new();
        let mut excludes = Vec::new();
        for rev in &args.revisions {
            if let Some(stripped) = rev.strip_prefix('^') {
                let oid = resolve_revision(&repo, stripped)?;
                excludes.push(oid);
            } else {
                match resolve_revision(&repo, rev) {
                    Ok(oid) => oids.push(oid),
                    Err(_err) if is_likely_pathspec_during_rev_parse(rev) => {
                        implied_pathspecs.push(rev.clone());
                    }
                    Err(err) => return Err(err),
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
    let source_map: std::collections::HashMap<ObjectId, String> = if args.source && args.all {
        build_source_map(&repo.odb, &repo.git_dir, args.first_parent)?
    } else {
        std::collections::HashMap::new()
    };

    // Compile filter regexes
    let author_re = args
        .author
        .as_ref()
        .map(|p| RegexBuilder::new(p).case_insensitive(true).build())
        .transpose()
        .context("invalid --author regex")?;
    let committer_re = args
        .committer_filter
        .as_ref()
        .map(|p| RegexBuilder::new(p).case_insensitive(true).build())
        .transpose()
        .context("invalid --committer regex")?;
    let grep_re = args
        .grep
        .as_ref()
        .map(|p| {
            let pattern = p.replace(r"\|", "|");
            RegexBuilder::new(&pattern)
                .case_insensitive(args.regexp_ignore_case)
                .build()
        })
        .transpose()
        .context("invalid --grep regex")?;

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

    // Collect ref decorations — manually determine last-wins for
    // --decorate / --no-decorate so that flag order is respected.
    let (show_decorations, decorate_full) = {
        // Default: decorations off, except when the chosen format asks for
        // `%d` / `%D` placeholders.
        let mut show = format_requires_decorations;
        let mut full = format_requires_decorations;
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
        }
        if args.no_decorate {
            show = false;
        }
        (show, full)
    };
    let decorations = if !show_decorations {
        None
    } else {
        Some(collect_decorations(&repo, decorate_full)?)
    };

    // Walk commits
    let mut combined_pathspecs = args.pathspecs.clone();
    combined_pathspecs.extend(implied_pathspecs.iter().cloned());
    combined_pathspecs = resolve_effective_pathspecs(&repo, &combined_pathspecs)?;

    let effective_pathspecs = if args.follow {
        &[][..]
    } else {
        &combined_pathspecs[..]
    };

    let find_oid = if let Some(ref find_obj_rev) = args.find_object {
        Some(resolve_revision(&repo, find_obj_rev)?)
    } else {
        None
    };
    let since_str = args.since_as_filter.as_ref().or(args.since.as_ref());
    let since_threshold = since_str.and_then(|s| parse_date_to_epoch(s));
    let until_threshold = args.until.as_ref().and_then(|s| parse_date_to_epoch(s));
    let diff_filter_str = args.diff_filter.as_deref();

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
        || args.stat
        || args.name_only
        || args.name_status
        || args.raw
        || args.cc;

    let mut notes_cache = NotesMapCache::new(&repo);
    let flush_each = out.is_terminal();

    if use_streaming_log {
        let mut iter = WalkCommitsIter::new(
            &repo,
            &repo.git_dir,
            &start_oids,
            if args.follow { None } else { args.max_count }, // follow needs full walk for rename tracking
            args.skip,
            args.first_parent,
            author_re.as_ref(),
            committer_re.as_ref(),
            grep_re.as_ref(),
            args.no_merges,
            args.merges,
            effective_pathspecs,
            &excluded_set,
        );
        let mut shown = 0usize;
        while let Some((oid, commit_data)) = iter.next_commit()? {
            if !commit_passes_post_walk_filters(
                &repo.odb,
                &oid,
                &commit_data,
                &args,
                diff_filter_str,
                find_oid,
                decorations.as_ref(),
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
            if args.source {
                if let Some(src) = source_map.get(&oid) {
                    let short_src = src
                        .strip_prefix("refs/heads/")
                        .or_else(|| src.strip_prefix("refs/tags/"))
                        .or_else(|| src.strip_prefix("refs/remotes/"))
                        .unwrap_or(src);
                    write!(out, "{}\t", short_src)?;
                }
            }
            format_commit(
                &mut out,
                &oid,
                &commit_data,
                &args,
                decorations.as_ref(),
                decoration_colors,
                use_color,
                &mut notes_cache,
                &repo.odb,
            )?;

            if show_diff {
                write_commit_diff(&mut out, &repo.odb, &commit_data, &args)?;
            }
            if flush_each {
                out.flush()?;
            }
            shown += 1;
        }
    } else {
        let commits = walk_commits(
            &repo,
            &repo.git_dir,
            &start_oids,
            if args.follow { None } else { args.max_count }, // follow needs full walk for rename tracking
            args.skip,
            args.first_parent,
            author_re.as_ref(),
            committer_re.as_ref(),
            grep_re.as_ref(),
            args.no_merges,
            args.merges,
            effective_pathspecs,
            &excluded_set,
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
                    commit_has_object(&repo.odb, info, &find_oid_buf).unwrap_or_default()
                })
                .collect::<Vec<_>>()
        } else {
            commits
        };

        // Apply --simplify-by-decoration: only show commits with decorations
        let commits = if args.simplify_by_decoration {
            match &decorations {
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

        for (i, (oid, commit_data)) in commits.iter().enumerate() {
            if is_format_separator && i > 0 {
                if args.null_terminator {
                    write!(out, "\0")?;
                } else {
                    writeln!(out)?;
                }
            }
            // Show --source annotation if available
            if args.source {
                if let Some(src) = source_map.get(oid) {
                    let short_src = src
                        .strip_prefix("refs/heads/")
                        .or_else(|| src.strip_prefix("refs/tags/"))
                        .or_else(|| src.strip_prefix("refs/remotes/"))
                        .unwrap_or(src);
                    write!(out, "{}\t", short_src)?;
                }
            }
            format_commit(
                &mut out,
                oid,
                commit_data,
                &args,
                decorations.as_ref(),
                decoration_colors,
                use_color,
                &mut notes_cache,
                &repo.odb,
            )?;

            if show_diff {
                write_commit_diff(&mut out, &repo.odb, commit_data, &args)?;
            }
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
        anyhow::bail!("pathspec '{}' is outside repository", pathspecs[0]);
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
fn run_no_walk(
    repo: &Repository,
    args: &Args,
    decoration_colors: Option<&DecorationColorStyle>,
) -> Result<()> {
    let mut oids = Vec::new();
    if args.revisions.is_empty() {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            oids.push(*oid);
        }
    } else {
        for rev in &args.revisions {
            let oid = resolve_revision(repo, rev)?;
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
        let ts_a = extract_timestamp(&a.1.committer);
        let ts_b = extract_timestamp(&b.1.committer);
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
        || args.stat
        || args.name_only
        || args.name_status
        || args.raw
        || args.cc;

    let mut notes_cache = NotesMapCache::new(repo);

    for (i, (oid, commit_data)) in commits.iter().enumerate() {
        if is_format_separator && i > 0 {
            writeln!(out)?;
        }
        format_commit(
            &mut out,
            oid,
            commit_data,
            args,
            decorations.as_ref(),
            decoration_colors,
            false,
            &mut notes_cache,
            &repo.odb,
        )?;
        if show_diff {
            write_commit_diff(&mut out, &repo.odb, commit_data, args)?;
        }
    }

    Ok(())
}

/// Run the reflog walk mode (`log -g` / `log --walk-reflogs`).
fn run_reflog_walk(repo: &Repository, args: &Args) -> Result<()> {
    // Determine which ref to walk
    let refname = if args.revisions.is_empty() {
        "HEAD".to_string()
    } else {
        let r = &args.revisions[0];
        if r == "HEAD" || r.starts_with("refs/") {
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

    for (i, entry) in entries.iter().rev().enumerate() {
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

        let selector = format!("{}@{{{}}}", display_name, i);

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
                write!(
                    out,
                    "{} {}@{{{}}}: {}\0",
                    abbrev, display_name, i, entry.message
                )?;
            } else {
                writeln!(
                    out,
                    "{} {}@{{{}}}: {}",
                    abbrev, display_name, i, entry.message
                )?;
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
            writeln!(out)?;
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
                    result.push_str(commit.message.trim());
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

/// Format date from ident for header display.
fn format_date_for_header(ident: &str) -> String {
    format_date_with_mode(ident, None)
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
    repo: &'a Repository,
    shallow_boundaries: HashSet<ObjectId>,
    visited: HashSet<ObjectId>,
    queue: std::collections::BinaryHeap<(i64, ObjectId)>,
    skipped: usize,
    skip_n: usize,
    max_count: Option<usize>,
    first_parent: bool,
    author_re: Option<&'a Regex>,
    committer_re: Option<&'a Regex>,
    grep_re: Option<&'a Regex>,
    no_merges: bool,
    merges_only: bool,
    pathspecs: &'a [String],
    accepted_count: usize,
}

impl<'a> WalkCommitsIter<'a> {
    fn new(
        repo: &'a Repository,
        git_dir: &Path,
        start: &[ObjectId],
        max_count: Option<usize>,
        skip: Option<usize>,
        first_parent: bool,
        author_re: Option<&'a Regex>,
        committer_re: Option<&'a Regex>,
        grep_re: Option<&'a Regex>,
        no_merges: bool,
        merges_only: bool,
        pathspecs: &'a [String],
        excluded: &HashSet<ObjectId>,
    ) -> Self {
        let shallow_boundaries = load_shallow_boundaries(git_dir);
        let visited: HashSet<ObjectId> = excluded.clone();
        let mut queue: std::collections::BinaryHeap<(i64, ObjectId)> =
            std::collections::BinaryHeap::new();
        for oid in start {
            let ts = read_commit_timestamp(repo, oid);
            queue.push((ts, *oid));
        }
        Self {
            repo,
            shallow_boundaries,
            visited,
            queue,
            skipped: 0,
            skip_n: skip.unwrap_or(0),
            max_count,
            first_parent,
            author_re,
            committer_re,
            grep_re,
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

            let obj = self.repo.read_replaced(&oid)?;
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
                        let ts = read_commit_timestamp(self.repo, parent);
                        self.queue.push((ts, *parent));
                    }
                } else {
                    for parent in &commit.parents {
                        if !self.visited.contains(parent) {
                            let ts = read_commit_timestamp(self.repo, parent);
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
            if let Some(re) = self.author_re {
                if !re.is_match(&info.author) {
                    continue;
                }
            }
            if let Some(re) = self.committer_re {
                if !re.is_match(&info.committer) {
                    continue;
                }
            }
            if let Some(re) = self.grep_re {
                if !re.is_match(&info.message) {
                    continue;
                }
            }
            if !self.pathspecs.is_empty()
                && !commit_touches_paths(&self.repo.odb, &info, self.pathspecs)?
            {
                continue;
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
    repo: &Repository,
    git_dir: &Path,
    start: &[ObjectId],
    max_count: Option<usize>,
    skip: Option<usize>,
    first_parent: bool,
    author_re: Option<&Regex>,
    committer_re: Option<&Regex>,
    grep_re: Option<&Regex>,
    no_merges: bool,
    merges_only: bool,
    pathspecs: &[String],
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<(ObjectId, CommitInfo)>> {
    if max_count == Some(0) {
        return Ok(Vec::new());
    }
    let mut iter = WalkCommitsIter::new(
        repo,
        git_dir,
        start,
        max_count,
        skip,
        first_parent,
        author_re,
        committer_re,
        grep_re,
        no_merges,
        merges_only,
        pathspecs,
        excluded,
    );
    let mut result = Vec::new();
    while let Some(c) = iter.next_commit()? {
        result.push(c);
    }
    Ok(result)
}

/// Check if a commit touches any of the given pathspecs by diffing against parents.
fn commit_touches_paths(odb: &Odb, info: &CommitInfo, pathspecs: &[String]) -> Result<bool> {
    if info.parents.is_empty() {
        // Root commit: diff against empty tree
        let entries = diff_trees(odb, None, Some(&info.tree), "")?;
        return Ok(entries.iter().any(|e| {
            let path = e.path();
            pathspecs.iter().any(|ps| path_matches(path, ps))
        }));
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
fn read_commit_timestamp(repo: &Repository, oid: &ObjectId) -> i64 {
    match repo.read_replaced(oid) {
        Ok(obj) => match parse_commit(&obj.data) {
            Ok(commit) => extract_timestamp(&commit.committer),
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

fn extract_timestamp(ident: &str) -> i64 {
    // Format: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() >= 2 {
        parts[1].parse().unwrap_or(0)
    } else {
        0
    }
}

/// Parse a timezone offset string like "+0200" or "-0500" into seconds.
fn parse_tz_offset(offset: &str) -> i64 {
    let bytes = offset.as_bytes();
    if bytes.len() < 5 {
        return 0;
    }
    let sign = if bytes[0] == b'-' { -1i64 } else { 1i64 };
    let hours: i64 = offset[1..3].parse().unwrap_or(0);
    let minutes: i64 = offset[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
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
    use grit_lib::objects::parse_tree;
    use grit_lib::refs::resolve_ref;

    let mut map = std::collections::HashMap::new();

    // Determine notes ref: check core.notesRef, GIT_NOTES_REF env, or default
    let notes_ref = std::env::var("GIT_NOTES_REF")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
            config
                .get("core.notesRef")
                .unwrap_or_else(|| "refs/notes/commits".to_string())
        });

    // Resolve notes ref to a commit, then get its tree
    let notes_oid = match resolve_ref(&repo.git_dir, &notes_ref) {
        Ok(oid) => oid,
        Err(_) => return map,
    };

    let obj = match repo.odb.read(&notes_oid) {
        Ok(o) => o,
        Err(_) => return map,
    };

    let tree_oid = match obj.kind {
        grit_lib::objects::ObjectKind::Commit => match parse_commit(&obj.data) {
            Ok(c) => c.tree,
            Err(_) => return map,
        },
        grit_lib::objects::ObjectKind::Tree => notes_oid,
        _ => return map,
    };

    let tree_obj = match repo.odb.read(&tree_oid) {
        Ok(o) => o,
        Err(_) => return map,
    };

    let entries = match parse_tree(&tree_obj.data) {
        Ok(e) => e,
        Err(_) => return map,
    };

    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        if let Ok(commit_oid) = name.parse::<ObjectId>() {
            // Read the blob to get note content
            if let Ok(blob) = repo.odb.read(&entry.oid) {
                map.insert(commit_oid, blob.data);
            }
        }
    }

    map
}

/// Write notes for a commit if any exist.
fn write_notes(
    out: &mut impl Write,
    oid: &ObjectId,
    notes_cache: &mut NotesMapCache<'_>,
    _odb: &Odb,
) -> Result<()> {
    let notes_map = notes_cache.map();
    if let Some(note_data) = notes_map.get(oid) {
        let note_text = String::from_utf8_lossy(note_data);
        writeln!(out)?;
        writeln!(out, "Notes:")?;
        for line in note_text.lines() {
            writeln!(out, "    {line}")?;
        }
    }
    Ok(())
}

/// Post-walk filters applied after [`walk_commits`] (diff-filter, find-object, decoration, dates).
fn commit_passes_post_walk_filters(
    odb: &Odb,
    oid: &ObjectId,
    info: &CommitInfo,
    args: &Args,
    diff_filter: Option<&str>,
    find_oid: Option<ObjectId>,
    decorations: Option<&HashMap<String, Vec<RefDecoration>>>,
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
        let passes = if !include_chars.is_empty() {
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
        if !commit_has_object(odb, info, &fo).unwrap_or_default() {
            return Ok(false);
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

/// Format and print a single commit.
fn format_commit(
    out: &mut impl Write,
    oid: &ObjectId,
    info: &CommitInfo,
    args: &Args,
    decorations: Option<&HashMap<String, Vec<RefDecoration>>>,
    decoration_colors: Option<&DecorationColorStyle>,
    use_color: bool,
    notes_cache: &mut NotesMapCache<'_>,
    odb: &Odb,
) -> Result<()> {
    let hex = oid.to_hex();
    let abbrev_len = effective_abbrev_len(args);

    if args.oneline || args.format.as_deref() == Some("oneline") {
        let first_line = info.message.lines().next().unwrap_or("");
        let dec_colors = if use_color { decoration_colors } else { None };
        let dec = format_decoration(&hex, decorations, dec_colors);
        let hash = &hex[..abbrev_len.min(hex.len())];
        if use_color {
            if let Some(c) = decoration_colors {
                write!(out, "{}{}{}", c.commit, hash, c.reset)?;
                if dec.is_empty() {
                    writeln!(out, " {first_line}")?;
                } else {
                    writeln!(out, "{dec} {first_line}")?;
                }
            } else {
                writeln!(out, "{hash}{dec} {first_line}")?;
            }
        } else {
            writeln!(out, "{hash}{dec} {first_line}")?;
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
            let dec_colors = if use_color { decoration_colors } else { None };
            let formatted = apply_format_string(
                template,
                oid,
                info,
                decorations,
                dec_colors,
                date_format,
                abbrev_len,
                use_color,
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
            let dec = format_decoration(&hex, decorations, None);
            writeln!(out, "commit {hex}{dec}")?;
            if info.parents.len() > 1 {
                let parent_abbrevs: Vec<String> = info
                    .parents
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
            let dec = format_decoration(&hex, decorations, None);
            if use_color {
                writeln!(out, "\x1b[33mcommit {hex}\x1b[m{dec}")?;
            } else {
                writeln!(out, "commit {hex}{dec}")?;
            }
            if info.parents.len() > 1 {
                let parent_abbrevs: Vec<String> = info
                    .parents
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
                format_date_with_mode(&info.author, date_format)
            )?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            write_notes(out, oid, notes_cache, odb)?;
            writeln!(out)?;
        }
        Some("full") => {
            let dec = format_decoration(&hex, decorations, None);
            writeln!(out, "commit {hex}{dec}")?;
            if info.parents.len() > 1 {
                let parent_abbrevs: Vec<String> = info
                    .parents
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
            write_notes(out, oid, notes_cache, odb)?;
            writeln!(out)?;
        }
        Some("fuller") => {
            let dec = format_decoration(&hex, decorations, None);
            writeln!(out, "commit {hex}{dec}")?;
            if info.parents.len() > 1 {
                let parent_abbrevs: Vec<String> = info
                    .parents
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
                format_date_with_mode(&info.author, date_format)
            )?;
            writeln!(out, "Commit:     {}", format_ident_display(&info.committer))?;
            writeln!(
                out,
                "CommitDate: {}",
                format_date_with_mode(&info.committer, date_format)
            )?;
            writeln!(out)?;
            for line in info.message.lines() {
                writeln!(out, "    {line}")?;
            }
            write_notes(out, oid, notes_cache, odb)?;
            writeln!(out)?;
        }
        Some(other) => {
            // Try as a format string directly
            let dec_colors = if use_color { decoration_colors } else { None };
            let formatted = apply_format_string(
                other,
                oid,
                info,
                decorations,
                dec_colors,
                date_format,
                abbrev_len,
                use_color,
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
    decorations: Option<&HashMap<String, Vec<RefDecoration>>>,
    decoration_colors: Option<&DecorationColorStyle>,
    date_format: Option<&str>,
    abbrev_len: usize,
    use_color: bool,
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
                    let parents: Vec<String> = info.parents.iter().map(|p| p.to_hex()).collect();
                    result.push_str(&parents.join(" "));
                }
                Some('p') => {
                    chars.next();
                    let parents: Vec<String> = info
                        .parents
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
                            result.push_str(&format!("{}", extract_timestamp(&info.author)));
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
                            result.push_str(&format!("{}", extract_timestamp(&info.committer)));
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
                    // Raw body: entire commit message
                    result.push_str(&info.message);
                }
                Some('d') => {
                    chars.next();
                    // Decorations
                    let dec_colors = if use_color { decoration_colors } else { None };
                    let dec = format_decoration(&hex, decorations, dec_colors);
                    result.push_str(&dec);
                }
                Some('D') => {
                    chars.next();
                    // Decorations without parens
                    let dec_colors = if use_color { decoration_colors } else { None };
                    let dec = format_decoration_no_parens(&hex, decorations, dec_colors);
                    result.push_str(&dec);
                }
                Some('n') => {
                    chars.next();
                    result.push('\n');
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
    // Git's `color_parse` orders SGR parameters: reset/clear codes first, then
    // attributes in a stable order (bold before reverse) so combined sequences
    // match upstream tests (`<BOLD;REVERSE;YELLOW>` not `<REVERSE;BOLD;YELLOW>`).
    let mut codes = Vec::new();
    let mut bold = false;
    let mut dim = false;
    let mut italic = false;
    let mut underline = false;
    let mut blink = false;
    let mut reverse = false;
    let mut strike = false;
    let mut nobold = false;
    let mut noitalic = false;
    let mut noul = false;
    let mut noblink = false;
    let mut noreverse = false;
    let mut nostrike = false;
    let mut fg: Option<u8> = None;
    let mut bg: Option<u8> = None;

    for part in spec.split_whitespace() {
        match part {
            "bold" => bold = true,
            "dim" => dim = true,
            "italic" => italic = true,
            "ul" | "underline" => underline = true,
            "blink" => blink = true,
            "reverse" => reverse = true,
            "strike" => strike = true,
            "nobold" | "nodim" => nobold = true,
            "noitalic" => noitalic = true,
            "noul" | "nounderline" => noul = true,
            "noblink" => noblink = true,
            "noreverse" => noreverse = true,
            "nostrike" => nostrike = true,
            _ => {
                if let Some(c) = color_code(part) {
                    if fg.is_none() {
                        fg = Some(c);
                    } else {
                        bg = Some(c);
                    }
                }
            }
        }
    }

    if nobold {
        codes.push("22".to_owned());
    }
    if noitalic {
        codes.push("23".to_owned());
    }
    if noul {
        codes.push("24".to_owned());
    }
    if noblink {
        codes.push("25".to_owned());
    }
    if noreverse {
        codes.push("27".to_owned());
    }
    if nostrike {
        codes.push("29".to_owned());
    }
    if bold {
        codes.push("1".to_owned());
    }
    if dim {
        codes.push("2".to_owned());
    }
    if italic {
        codes.push("3".to_owned());
    }
    if underline {
        codes.push("4".to_owned());
    }
    if blink {
        codes.push("5".to_owned());
    }
    if reverse {
        codes.push("7".to_owned());
    }
    if strike {
        codes.push("9".to_owned());
    }
    if let Some(c) = fg {
        codes.push(format!("{}", 30 + c));
    }
    if let Some(c) = bg {
        codes.push(format!("{}", 40 + c));
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
fn format_date_with_mode(ident: &str, date_mode: Option<&str>) -> String {
    // Git ident: "Name <email> timestamp offset"
    let parts: Vec<&str> = ident.rsplitn(3, ' ').collect();
    if parts.len() < 2 {
        return ident.to_owned();
    }
    let ts_str = parts[1];
    let offset_str = parts[0];
    let ts = match ts_str.parse::<i64>() {
        Ok(v) => v,
        Err(_) => return format!("{ts_str} {offset_str}"),
    };

    let tz_offset_secs = parse_tz_offset(offset_str);

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
            // Show relative time like "2 hours ago", "3 days ago"
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let diff = now - ts;
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
            // Default Git date format: "Thu Apr  7 15:13:13 2005 -0700"
            // Note: day is right-justified in a 2-char field (space-padded)
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
                "{} {} {:>2} {:02}:{:02}:{:02} {} {}",
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

/// Format the date from an ident string (legacy, default mode).
/// Resolve a revision string to an ObjectId.
fn resolve_revision(repo: &Repository, rev: &str) -> Result<ObjectId> {
    // Delegate to the library's full revision parser which handles
    // @{N}, @{now}, @{upstream}, peeling, parent navigation, etc.
    grit_lib::rev_parse::resolve_revision(repo, rev)
        .map_err(|e| anyhow::anyhow!("unknown revision '{}': {}", rev, e))
}

/// Heuristic used for rev/pathspec DWIM when no `--` separator is present.
fn is_likely_pathspec_during_rev_parse(token: &str) -> bool {
    if token.contains("^{") || token.contains("@{") || token.contains("..") {
        return false;
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

/// Collect ref decorations keyed by commit hex (short or full per caller).
fn collect_decorations(
    repo: &Repository,
    full: bool,
) -> Result<HashMap<String, Vec<RefDecoration>>> {
    let mut map: HashMap<String, Vec<RefDecoration>> = HashMap::new();
    let mut head_branch_full: Option<String> = None;

    let head = resolve_head(&repo.git_dir)?;
    if let Some(oid) = head.oid() {
        let hex = oid.to_hex();
        match &head {
            HeadState::Branch { short_name, .. } => {
                let branch_full = format!("refs/heads/{short_name}");
                head_branch_full = Some(branch_full.clone());
                map.entry(hex).or_default().push(RefDecoration {
                    kind: DecorationKind::Head,
                    full_name: branch_full,
                });
            }
            _ => {
                map.entry(hex).or_default().push(RefDecoration {
                    kind: DecorationKind::Head,
                    full_name: "HEAD".to_owned(),
                });
            }
        }
    }

    for (refname, oid) in list_refs(&repo.git_dir, "refs/heads/")? {
        let hex = oid.to_hex();
        map.entry(hex).or_default().push(RefDecoration {
            kind: DecorationKind::LocalBranch,
            full_name: refname,
        });
    }

    for (refname, oid) in list_refs(&repo.git_dir, "refs/tags/")? {
        let oid_hex = oid.to_hex();
        let hex = peel_to_commit_hex(&repo.odb, &oid_hex).unwrap_or(oid_hex);
        map.entry(hex).or_default().push(RefDecoration {
            kind: DecorationKind::Tag,
            full_name: refname,
        });
    }

    for (refname, oid) in list_refs(&repo.git_dir, "refs/remotes/")? {
        let hex = oid.to_hex();
        map.entry(hex).or_default().push(RefDecoration {
            kind: DecorationKind::Remote,
            full_name: refname,
        });
    }

    if let Ok(oid) = grit_lib::refs::resolve_ref(&repo.git_dir, "refs/stash") {
        map.entry(oid.to_hex()).or_default().push(RefDecoration {
            kind: DecorationKind::Stash,
            full_name: "refs/stash".to_owned(),
        });
    }

    if std::env::var_os("GIT_NO_REPLACE_OBJECTS").is_none() {
        let base = replace_ref_base();
        for (refname, _) in list_refs(&repo.git_dir, &base)? {
            let suffix = refname.strip_prefix(&base).unwrap_or("");
            if suffix.len() != 40 || !suffix.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            if let Ok(original) = suffix.parse::<ObjectId>() {
                map.entry(original.to_hex())
                    .or_default()
                    .push(RefDecoration {
                        kind: DecorationKind::Grafted,
                        full_name: "replaced".to_owned(),
                    });
            }
        }
    }

    if let Some(branch_full) = head_branch_full {
        if let Some(head_oid) = head.oid() {
            let head_hex = head_oid.to_hex();
            if let Some(list) = map.get_mut(&head_hex) {
                list.retain(|d| {
                    !(d.kind == DecorationKind::LocalBranch && d.full_name == branch_full)
                });
            }
        }
    }

    for list in map.values_mut() {
        list.sort_by(|a, b| {
            kind_sort_key(a.kind)
                .cmp(&kind_sort_key(b.kind))
                .then_with(|| match (a.kind, b.kind) {
                    (DecorationKind::Tag, DecorationKind::Tag) => b.full_name.cmp(&a.full_name),
                    (DecorationKind::Remote, DecorationKind::Remote) => {
                        b.full_name.cmp(&a.full_name)
                    }
                    _ => a.full_name.cmp(&b.full_name),
                })
        });
        list.dedup_by(|a, b| a.kind == b.kind && a.full_name == b.full_name);
    }

    if full {
        for list in map.values_mut() {
            for d in list.iter_mut() {
                if d.kind == DecorationKind::Head && d.full_name.starts_with("refs/heads/") {
                    d.full_name = format!("HEAD -> {}", d.full_name);
                }
            }
        }
    } else {
        for list in map.values_mut() {
            for d in list.iter_mut() {
                match d.kind {
                    DecorationKind::Head if d.full_name.starts_with("refs/heads/") => {
                        let short = d
                            .full_name
                            .strip_prefix("refs/heads/")
                            .unwrap_or(&d.full_name);
                        d.full_name = format!("HEAD -> {short}");
                    }
                    DecorationKind::Head => {}
                    DecorationKind::Tag => {
                        let short = d
                            .full_name
                            .strip_prefix("refs/tags/")
                            .unwrap_or(&d.full_name);
                        d.full_name = short.to_owned();
                    }
                    _ => {
                        d.full_name = prettify_refname(&d.full_name).to_owned();
                    }
                }
            }
        }
    }

    Ok(map)
}

fn kind_sort_key(k: DecorationKind) -> u8 {
    match k {
        DecorationKind::Head => 0,
        DecorationKind::LocalBranch => 1,
        DecorationKind::Tag => 2,
        DecorationKind::Remote => 3,
        DecorationKind::Stash => 4,
        DecorationKind::Grafted => 5,
    }
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

fn current_branch_index_for_head(list: &[RefDecoration]) -> Option<usize> {
    let head = list.iter().find(|d| d.kind == DecorationKind::Head)?;
    let rest = head.full_name.strip_prefix("HEAD -> ")?;
    let target = rest.trim();
    list.iter()
        .position(|d| d.kind == DecorationKind::LocalBranch && d.full_name == target)
}

fn format_decoration_inner(
    list: &[RefDecoration],
    colors: Option<&DecorationColorStyle>,
    with_outer_commit_parens: bool,
) -> String {
    if list.is_empty() {
        return String::new();
    }
    let skip_branch = current_branch_index_for_head(list);

    let mut parts_plain: Vec<String> = Vec::new();
    for (i, d) in list.iter().enumerate() {
        if skip_branch == Some(i) {
            continue;
        }
        let mut s = String::new();
        if d.kind == DecorationKind::Tag {
            s.push_str("tag: ");
            s.push_str(&d.full_name);
        } else {
            s.push_str(&d.full_name);
        }
        if d.kind == DecorationKind::Head && d.full_name.starts_with("HEAD -> ") {
            if let Some(bi) = skip_branch {
                s.push_str(" -> ");
                s.push_str(&list[bi].full_name);
            }
        }
        parts_plain.push(s);
    }

    let Some(c) = colors else {
        let inner = parts_plain.join(", ");
        if with_outer_commit_parens {
            return format!(" ({inner})");
        }
        return inner;
    };

    let mut out = String::new();
    let mut sep = if with_outer_commit_parens { " (" } else { "" };

    for (i, d) in list.iter().enumerate() {
        if skip_branch == Some(i) {
            continue;
        }
        out.push_str(&c.commit);
        out.push_str(sep);
        out.push_str(&c.reset);
        sep = ", ";

        if d.kind == DecorationKind::Tag {
            out.push_str(&c.tag);
            out.push_str("tag: ");
            out.push_str(&c.reset);
            out.push_str(&c.tag);
            out.push_str(&d.full_name);
            out.push_str(&c.reset);
        } else if d.kind == DecorationKind::Head && d.full_name.starts_with("HEAD -> ") {
            let branch_name = d.full_name.strip_prefix("HEAD -> ").unwrap_or(&d.full_name);
            out.push_str(&c.head);
            out.push_str("HEAD");
            out.push_str(&c.reset);
            out.push_str(&c.commit);
            out.push_str(" -> ");
            out.push_str(&c.reset);
            out.push_str(c.slot(DecorationKind::LocalBranch));
            out.push_str(branch_name);
            out.push_str(&c.reset);
        } else {
            out.push_str(c.slot(d.kind));
            out.push_str(&d.full_name);
            out.push_str(&c.reset);
        }
    }

    if with_outer_commit_parens {
        out.push_str(&c.commit);
        out.push(')');
        out.push_str(&c.reset);
    }

    out
}

/// Format decoration string for a commit (with parentheses).
fn format_decoration(
    hex: &str,
    decorations: Option<&HashMap<String, Vec<RefDecoration>>>,
    colors: Option<&DecorationColorStyle>,
) -> String {
    let Some(map) = decorations else {
        return String::new();
    };
    let Some(list) = map.get(hex) else {
        return String::new();
    };
    if list.is_empty() {
        return String::new();
    }
    format_decoration_inner(list, colors, true)
}

/// Format decoration string without parentheses (for %D).
fn format_decoration_no_parens(
    hex: &str,
    decorations: Option<&HashMap<String, Vec<RefDecoration>>>,
    colors: Option<&DecorationColorStyle>,
) -> String {
    let Some(map) = decorations else {
        return String::new();
    };
    let Some(list) = map.get(hex) else {
        return String::new();
    };
    if list.is_empty() {
        return String::new();
    }
    format_decoration_inner(list, colors, false)
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

/// Write diff output for a single commit.
fn write_commit_diff(
    out: &mut impl Write,
    odb: &Odb,
    info: &CommitInfo,
    args: &Args,
) -> Result<()> {
    let is_merge = info.parents.len() > 1;
    let mut entries = compute_commit_diff(odb, info)?;
    if entries.is_empty() {
        return Ok(());
    }

    // Apply orderfile sorting if specified
    if let Some(ref order_path) = args.order_file {
        entries = crate::commands::diff::apply_orderfile_entries(entries, order_path);
    }

    // For --cc mode on merge commits, compute combined diff entries
    // (only files that differ from ALL parents).
    let combined_entries = if args.cc && is_merge {
        compute_combined_diff_entries(odb, info)?
    } else {
        entries.clone()
    };

    // Determine if patch content will be shown (for --- separator logic)
    let has_patch = (args.patch || args.patch_u || args.cc) && {
        let show_entries = if args.cc && is_merge {
            &combined_entries
        } else {
            &entries
        };
        !show_entries.is_empty()
    };

    if args.raw {
        let show_entries = if args.cc && is_merge {
            &combined_entries
        } else {
            &entries
        };
        for entry in show_entries {
            writeln!(out, "{}", format_raw(entry))?;
        }
        writeln!(out)?;
    }

    // Print --- separator when stat + patch are both shown
    if args.stat {
        if has_patch {
            writeln!(out, "---")?;
        } else {
            writeln!(out)?;
        }
        log_print_stat_summary(out, odb, &entries, has_patch)?;
    }

    if args.name_only {
        let show_entries = if args.cc && is_merge {
            &combined_entries
        } else {
            &entries
        };
        for entry in show_entries {
            writeln!(out, "{}", entry.path())?;
        }
        writeln!(out)?;
    }

    if args.name_status {
        let show_entries = if args.cc && is_merge {
            &combined_entries
        } else {
            &entries
        };
        for entry in show_entries {
            writeln!(out, "{}\t{}", entry.status.letter(), entry.path())?;
        }
        writeln!(out)?;
    }

    if args.patch || args.patch_u || args.cc {
        let show_entries = if args.cc && is_merge {
            &combined_entries
        } else {
            &entries
        };
        for entry in show_entries {
            log_write_patch_entry(out, odb, entry, args.unified.unwrap_or(3))?;
        }
    }

    Ok(())
}

/// Write a unified-diff block for one entry.
fn log_write_patch_entry(
    out: &mut impl Write,
    odb: &Odb,
    entry: &DiffEntry,
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

    writeln!(out, "diff --git a/{old_path} b/{new_path}")?;

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

    let (old_content, new_content) = log_read_blob_pair(odb, entry)?;
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
    let patch = unified_diff(
        &old_content,
        &new_content,
        display_old,
        display_new,
        context_lines,
    );
    write!(out, "{patch}")?;

    Ok(())
}

/// Write a `--stat` summary for log.
fn log_print_stat_summary(
    out: &mut impl Write,
    odb: &Odb,
    entries: &[DiffEntry],
    trailing_blank: bool,
) -> Result<()> {
    let max_path_len = entries.iter().map(|e| e.path().len()).max().unwrap_or(0);
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for entry in entries {
        let (old_content, new_content) = log_read_blob_pair(odb, entry)?;
        let (ins, del) = count_changes(&old_content, &new_content);
        total_ins += ins;
        total_del += del;
        writeln!(
            out,
            "{}",
            format_stat_line(entry.path(), ins, del, max_path_len)
        )?;
    }

    let n = entries.len();
    let mut summary = format!(" {} file{} changed", n, if n == 1 { "" } else { "s" },);
    if total_ins > 0 {
        summary.push_str(&format!(
            ", {} insertion{}(+)",
            total_ins,
            if total_ins == 1 { "" } else { "s" },
        ));
    }
    if total_del > 0 {
        summary.push_str(&format!(
            ", {} deletion{}(-)",
            total_del,
            if total_del == 1 { "" } else { "s" },
        ));
    }
    writeln!(out, "{summary}")?;
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

/// Check whether a commit's diff introduces or removes a specific object.
fn commit_has_object(odb: &Odb, info: &CommitInfo, target: &ObjectId) -> Result<bool> {
    let parent_tree = if let Some(parent) = info.parents.first() {
        let pobj = odb.read(parent)?;
        let pc = parse_commit(&pobj.data)?;
        Some(pc.tree)
    } else {
        None
    };

    let entries = diff_trees(odb, parent_tree.as_ref(), Some(&info.tree), "")?;
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

/// Parse the `--abbrev` value into a hash abbreviation length.
fn parse_abbrev(abbrev: &Option<String>) -> usize {
    match abbrev {
        Some(val) => val.parse::<usize>().unwrap_or(7),
        None => 7,
    }
}

fn effective_abbrev_len(args: &Args) -> usize {
    if args.no_abbrev {
        40
    } else {
        parse_abbrev(&args.abbrev)
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
