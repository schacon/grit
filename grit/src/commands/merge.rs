//! `grit merge` — join two or more development histories together.
//!
//! Implements fast-forward, three-way merge with conflict handling,
//! `--squash`, `--no-ff`, `--ff-only`, `--abort`, and `--continue`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use grit_lib::config::ConfigSet;
use grit_lib::crlf::MergeAttr;
use grit_lib::diff::{count_changes, detect_renames, diff_trees, DiffEntry, DiffStatus};
use grit_lib::hooks::run_hook;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_GITLINK, MODE_SYMLINK, MODE_TREE};
use grit_lib::merge_base::is_ancestor;
use grit_lib::merge_file::{self, ConflictStyle, MergeFavor, MergeInput};
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::refs::resolve_ref;
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;
use time::OffsetDateTime;

use crate::commands::diff_index;
use crate::explicit_exit::ExplicitExit;

/// Run Git's `post-merge` hook with one argument: `0` for a normal merge, `1` for squash.
///
/// Matches upstream `merge.c` `finish()`: hook failures are ignored (Git does not abort the merge).
fn run_post_merge_hook(repo: &Repository, squash: bool) {
    let flag = if squash { "1" } else { "0" };
    let _ = run_hook(repo, "post-merge", &[flag], None);
}

/// Arguments for `grit merge`.
#[derive(Debug, Clone, ClapArgs)]
#[command(about = "Join two or more development histories together")]
pub struct Args {
    /// Branch or commit to merge.
    #[arg(value_name = "COMMIT")]
    pub commits: Vec<String>,

    /// Custom merge commit message.
    #[arg(short = 'm', long = "message")]
    pub message: Option<String>,

    /// Only allow fast-forward merges.
    #[arg(long = "ff-only")]
    pub ff_only: bool,

    /// Always create a merge commit (no fast-forward).
    #[arg(long = "no-ff")]
    pub no_ff: bool,

    /// Perform the merge but don't commit.
    #[arg(long = "no-commit")]
    pub no_commit: bool,

    /// Squash merge: stage changes but don't commit.
    #[arg(long = "squash")]
    pub squash: bool,

    /// Abort in-progress merge.
    #[arg(long = "abort")]
    pub abort: bool,

    /// Continue after resolving conflicts.
    #[arg(long = "continue")]
    pub continue_merge: bool,

    /// Merge strategy to use (e.g. recursive, ort, resolve, octopus, ours).
    /// May be passed multiple times (`-s ort -s octopus`); each is tried in order until one succeeds.
    #[arg(short = 's', long = "strategy", action = clap::ArgAction::Append)]
    pub strategy: Vec<String>,

    /// Strategy-specific option (e.g. ours, theirs).
    #[arg(short = 'X', long = "strategy-option")]
    pub strategy_option: Vec<String>,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Force progress reporting.
    #[arg(long = "progress")]
    pub progress: bool,

    /// Suppress progress reporting.
    #[arg(long = "no-progress")]
    pub no_progress: bool,

    /// Allow merging histories that do not share a common ancestor.
    #[arg(long = "allow-unrelated-histories")]
    pub allow_unrelated_histories: bool,

    /// Suppress editor launch for the merge commit message.
    #[arg(long = "no-edit")]
    pub no_edit: bool,

    /// Open editor for the merge commit message (default for non-automated merges).
    #[arg(long = "edit", short = 'e')]
    pub edit: bool,

    /// Add Signed-off-by trailer to the merge commit message.
    #[arg(short = 'S', long = "signoff")]
    pub signoff: bool,

    /// Do not add Signed-off-by trailer.
    #[arg(long = "no-signoff")]
    pub no_signoff: bool,

    /// Show a diffstat at the end of the merge.
    #[arg(long = "stat")]
    pub stat: bool,

    /// Synonym for --stat.
    #[arg(short = 'n', long = "no-stat")]
    pub no_stat: bool,

    /// Show log messages from commits being merged.
    #[arg(long = "log", value_name = "N", num_args = 0..=1, default_missing_value = "20", require_equals = true)]
    pub log: Option<usize>,

    /// Do not include log messages.
    #[arg(long = "no-log")]
    pub no_log: bool,

    /// Show compact-summary in diffstat output.
    #[arg(long = "compact-summary")]
    pub compact_summary: bool,

    /// Show summary (deprecated synonym for --stat).
    #[arg(long = "summary")]
    pub summary: bool,

    /// Allow fast-forward (default).
    #[arg(long = "ff")]
    pub ff: bool,

    /// Allow fast-forward (aliases for configuration).
    #[arg(long = "commit")]
    pub commit: bool,

    /// Undo --squash.
    #[arg(long = "no-squash")]
    pub no_squash: bool,

    /// Quit merge.
    #[arg(long = "quit")]
    pub quit: bool,

    /// Automatically stash/unstash before/after merge.
    #[arg(long = "autostash")]
    pub autostash: bool,

    /// How to clean up the merge message.
    #[arg(long = "cleanup", value_name = "MODE")]
    pub cleanup: Option<String>,

    /// Read the commit message from the given file.
    #[arg(short = 'F', long = "file", value_name = "FILE")]
    pub file: Option<String>,

    /// After a failed merge, record conflict preimages / replay recorded resolutions and optionally stage.
    #[arg(long = "rerere-autoupdate")]
    pub rerere_autoupdate: bool,

    /// Do not update the index when a recorded rerere resolution is replayed.
    #[arg(long = "no-rerere-autoupdate")]
    pub no_rerere_autoupdate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtreeShift {
    Disabled,
    Auto,
    Prefix(String),
}

/// First `-s` strategy wins for merge-tree behavior; empty means default (ort).
fn primary_merge_strategy(args: &Args) -> Option<&str> {
    args.strategy.first().map(String::as_str)
}

/// Subtree path shifting for `merge_trees`: `-s subtree` implies auto-detect unless `-X subtree` set a prefix.
fn effective_subtree_shift(
    primary_strategy: Option<&str>,
    configured: &SubtreeShift,
) -> SubtreeShift {
    if primary_strategy == Some("subtree") {
        match configured {
            SubtreeShift::Disabled => SubtreeShift::Auto,
            other => other.clone(),
        }
    } else {
        configured.clone()
    }
}

/// True when every stage-0 index entry matches `HEAD^{tree}` and every HEAD path is present
/// in the index (no staged add/delete/modify vs HEAD).
///
/// Uses entry-by-entry comparison — not `write_tree_from_index` — so intent-to-add and other
/// entries omitted from the written tree still make the index "dirty" vs HEAD when appropriate.
/// Like [`index_matches_head_tree`] but compares the index to an arbitrary commit's tree.
fn index_matches_commit_tree(repo: &Repository, commit_oid: ObjectId) -> Result<bool> {
    let index = repo.load_index()?;
    let tree_oid = commit_tree(repo, commit_oid)?;
    let tree_entries = tree_to_map(tree_to_index_entries(repo, &tree_oid, "")?);
    let mut index_paths: BTreeSet<Vec<u8>> = BTreeSet::new();
    for e in index.entries.iter().filter(|e| e.stage() == 0) {
        index_paths.insert(e.path.clone());
        match tree_entries.get(&e.path) {
            Some(te) => {
                if te.oid != e.oid || te.mode != e.mode {
                    return Ok(false);
                }
            }
            None => return Ok(false),
        }
    }
    for path in tree_entries.keys() {
        if !index_paths.contains(path) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn index_matches_head_tree(repo: &Repository, head_oid: ObjectId) -> Result<bool> {
    let index = repo.load_index()?;
    let head_tree = commit_tree(repo, head_oid)?;
    let head_entries = tree_to_map(tree_to_index_entries(repo, &head_tree, "")?);
    let mut index_paths: BTreeSet<Vec<u8>> = BTreeSet::new();
    for e in index.entries.iter().filter(|e| e.stage() == 0) {
        index_paths.insert(e.path.clone());
        match head_entries.get(&e.path) {
            Some(he) => {
                if he.oid != e.oid || he.mode != e.mode {
                    return Ok(false);
                }
            }
            None => return Ok(false),
        }
    }
    for path in head_entries.keys() {
        if !index_paths.contains(path) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Refuse merge when the index does not match `HEAD^{tree}`. Used for octopus and for
/// recursive/ort/subtree single-parent merges (t6424). `--autostash` skips this check.
fn bail_if_index_tree_differs_from_head(
    repo: &Repository,
    head_oid: ObjectId,
    autostash: bool,
) -> Result<()> {
    if autostash {
        return Ok(());
    }
    if index_matches_head_tree(repo, head_oid)? {
        return Ok(());
    }
    bail!(
        "Your local changes to the following files would be overwritten by merge:\n\
         \t(index does not match HEAD)\n\
         Please commit your changes or stash them before you merge.\n\
         Aborting"
    );
}

/// The resolve strategy does not allow *any* staged difference from HEAD (including removals),
/// unlike recursive/ort which only care when the merge would touch those paths.
fn bail_if_resolve_index_not_clean_vs_head(
    repo: &Repository,
    head_oid: ObjectId,
    autostash: bool,
) -> Result<()> {
    if autostash {
        return Ok(());
    }
    let index = repo.load_index()?;
    let head_tree = commit_tree(repo, head_oid)?;
    let head_entries = tree_to_map(tree_to_index_entries(repo, &head_tree, "")?);
    let mut dirty_paths: BTreeSet<String> = BTreeSet::new();
    for e in index.entries.iter().filter(|e| e.stage() == 0) {
        let rel = String::from_utf8_lossy(&e.path).to_string();
        match head_entries.get(&e.path) {
            Some(he) => {
                if he.oid != e.oid || he.mode != e.mode {
                    dirty_paths.insert(rel);
                }
            }
            None => {
                dirty_paths.insert(rel);
            }
        }
    }
    for path in head_entries.keys() {
        if !index
            .entries
            .iter()
            .any(|e| e.stage() == 0 && e.path == *path)
        {
            dirty_paths.insert(String::from_utf8_lossy(path).to_string());
        }
    }
    if dirty_paths.is_empty() {
        return Ok(());
    }
    let mut msg =
        String::from("Your local changes to the following files would be overwritten by merge:\n");
    for path in &dirty_paths {
        msg.push_str(&format!("\t{path}\n"));
    }
    msg.push_str("Please commit your changes or stash them before you merge.\nAborting");
    bail!("{msg}");
}

/// Fast-forward index: the merge target tree plus **unrelated** staged additions (paths not in
/// `HEAD^{tree}` and not in the target tree). Paths present in HEAD but absent from the target
/// (deletes/renames) must not be copied from the index — only the target layout wins.
fn compose_fast_forward_index(
    repo: &Repository,
    target_tree: ObjectId,
    head_tree: ObjectId,
    current_index: &Index,
) -> Result<Index> {
    let mut new_entries = tree_to_index_entries(repo, &target_tree, "")?;
    let target_paths: BTreeSet<Vec<u8>> = new_entries.iter().map(|e| e.path.clone()).collect();
    let head_entries = tree_to_map(tree_to_index_entries(repo, &head_tree, "")?);
    for e in &current_index.entries {
        if e.stage() != 0 {
            continue;
        }
        if target_paths.contains(&e.path) {
            continue;
        }
        // Staged addition: not in HEAD — keep alongside the fast-forwarded tree.
        if !head_entries.contains_key(&e.path) {
            new_entries.push(e.clone());
        }
    }
    let mut index = Index::new();
    index.entries = new_entries;
    index.sort();
    Ok(index)
}

/// Preserve staged paths from before an octopus merge that the merge result does not touch
/// (e.g. unrelated `git add`), matching Git's index composition.
fn compose_octopus_final_index(pre_merge_index: &Index, final_index: &mut Index) {
    let final_paths: BTreeSet<Vec<u8>> = final_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    for e in &pre_merge_index.entries {
        if e.stage() != 0 {
            continue;
        }
        if final_paths.contains(&e.path) {
            continue;
        }
        final_index.entries.push(e.clone());
    }
    final_index.sort();
}

/// Restore index and working tree to match `head_oid` after a failed merge attempt
/// (used when trying multiple `-s` strategies so a failed strategy leaves no residue).
/// Write `index_snapshot` to disk and refresh the work tree (clears merge state files).
fn restore_index_and_worktree(repo: &Repository, index_snapshot: &Index) -> Result<()> {
    let _ = fs::remove_file(repo.git_dir.join("MERGE_HEAD"));
    let _ = fs::remove_file(repo.git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(repo.git_dir.join("MERGE_MODE"));
    let mut index = Index::new();
    index.entries = index_snapshot.entries.clone();
    index.sort();
    if let Some(ref wt) = repo.work_tree {
        checkout_entries(repo, wt, &index, None)?;
    }
    repo.write_index(&mut index)?;
    Ok(())
}

fn restore_repo_to_head(repo: &Repository, head_oid: ObjectId) -> Result<()> {
    let commit_obj = repo.odb.read(&head_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let entries = tree_to_index_entries(repo, &commit.tree, "")?;
    let mut index = Index::new();
    index.entries = entries;
    index.sort();
    restore_index_and_worktree(repo, &index)
}

/// Apply branch.<name>.mergeoptions to the args.
/// Only applies settings that weren't explicitly set on the command line.
fn apply_mergeoptions(args: &mut Args, opts: &str) {
    // Save CLI-set flags before applying config options
    let cli_ff = args.ff;
    let cli_no_ff = args.no_ff;
    let cli_ff_only = args.ff_only;
    let cli_squash = args.squash;
    let cli_no_squash = args.no_squash;
    let cli_commit = args.commit;
    let cli_no_commit = args.no_commit;
    let cli_stat = args.stat;
    let cli_no_stat = args.no_stat;
    let cli_summary = args.summary;

    for token in opts.split_whitespace() {
        match token {
            "--ff" if !cli_no_ff && !cli_ff_only => args.ff = true,
            "--no-ff" if !cli_ff && !cli_ff_only => args.no_ff = true,
            "--ff-only" if !cli_ff && !cli_no_ff => args.ff_only = true,
            "--squash" if !cli_no_squash => args.squash = true,
            "--no-squash" if !cli_squash => args.no_squash = true,
            "--commit" if !cli_no_commit => args.commit = true,
            "--no-commit" if !cli_commit => args.no_commit = true,
            "--stat" if !cli_no_stat => args.stat = true,
            "--no-stat" | "-n" if !cli_stat && !cli_summary => args.no_stat = true,
            "--log" => {
                if args.log.is_none() {
                    args.log = Some(20);
                }
            }
            "--no-log" => args.no_log = true,
            "--signoff" | "-S" if !args.no_signoff => args.signoff = true,
            "--no-signoff" if !args.signoff => args.no_signoff = true,
            "--edit" | "-e" if !args.no_edit => args.edit = true,
            "--no-edit" if !args.edit => args.no_edit = true,
            "--quiet" | "-q" => args.quiet = true,
            "--summary" if !cli_no_stat => args.summary = true,
            "--rerere-autoupdate" => args.rerere_autoupdate = true,
            "--no-rerere-autoupdate" => args.no_rerere_autoupdate = true,
            _ => {} // ignore unknown options
        }
    }
}

/// Run the `merge` command.
pub fn run(mut args: Args) -> Result<()> {
    if args.abort {
        return merge_abort();
    }
    if args.continue_merge {
        return merge_continue(args.message);
    }

    // Handle -s help early (before commit check)
    if args.strategy.iter().any(|s| s == "help") {
        eprintln!("Could not find merge strategy 'help'.");
        eprintln!("Available strategies are: octopus ours recursive resolve subtree theirs.");
        std::process::exit(1);
    }

    if args.quit {
        return merge_quit();
    }

    if args.commits.is_empty() {
        bail!("nothing to merge — please specify a branch or commit");
    }

    // Read merge.ff config and apply unless overridden by CLI flags.
    // CLI flags (--ff, --no-ff, --ff-only) take precedence over config.
    let repo = Repository::discover(None).context("not a git repository")?;
    if args.commits.len() == 1 && args.commits[0] == "FETCH_HEAD" {
        args.commits = read_fetch_head_merge_oids(&repo)?;
    }
    let mut merge_renormalize = false;
    {
        let config = ConfigSet::load(Some(&repo.git_dir), true)?;

        // Read branch.<name>.mergeoptions and apply them (CLI flags override these).
        let head_state = resolve_head(&repo.git_dir)?;
        if let Some(branch_name) = head_state.branch_name() {
            let key = format!("branch.{branch_name}.mergeoptions");
            if let Some(opts) = config.get(&key) {
                apply_mergeoptions(&mut args, &opts);
            }
        }

        if !args.ff && !args.no_ff && !args.ff_only {
            if let Some(val) = config.get("merge.ff") {
                match val.to_lowercase().as_str() {
                    "false" | "no" => args.no_ff = true,
                    "only" => args.ff_only = true,
                    _ => {} // "true" or anything else = default (allow ff)
                }
            }
        }
        if let Some(value) = config.get_bool("merge.renormalize") {
            merge_renormalize = value.unwrap_or(false);
        }
        // Read merge.log config
        if args.log.is_none() && !args.no_log {
            if let Some(val) = config.get("merge.log") {
                match val.to_lowercase().as_str() {
                    "true" | "yes" => args.log = Some(20),
                    "false" | "no" => {}
                    _ => {
                        if let Ok(n) = val.parse::<usize>() {
                            if n > 0 {
                                args.log = Some(n);
                            }
                        }
                    }
                }
            }
        }
        // Read merge.stat config
        if !args.stat && !args.no_stat {
            if let Some(val) = config.get("merge.stat") {
                match val.to_lowercase().as_str() {
                    "true" | "yes" => args.stat = true,
                    "compact" => {
                        args.stat = true;
                        args.compact_summary = true;
                    }
                    _ => {}
                }
            }
        }
    }

    exit_if_merge_blocked_by_index_or_state(&repo)?;

    if args.squash && args.no_ff {
        bail!("fatal: You cannot combine --squash with --no-ff.");
    }
    if args.squash && args.commit {
        bail!("fatal: You cannot combine --squash with --commit.");
    }

    // Validate --strategy: accept known names for every `-s` occurrence.
    for strat in &args.strategy {
        match strat.as_str() {
            "recursive" | "ort" | "resolve" | "octopus" | "ours" | "theirs" | "subtree" => {}
            other => bail!("Could not find merge strategy '{}'", other),
        }
    }

    // Parse -X strategy options
    let mut favor = MergeFavor::None;
    let mut diff_algorithm: Option<String> = None;
    let mut subtree_shift = SubtreeShift::Disabled;
    for xopt in &args.strategy_option {
        if let Some(algo) = xopt.strip_prefix("diff-algorithm=") {
            diff_algorithm = Some(algo.to_string());
        } else if xopt == "renormalize" {
            merge_renormalize = true;
        } else if xopt == "no-renormalize" {
            merge_renormalize = false;
        } else if xopt == "subtree" {
            subtree_shift = SubtreeShift::Auto;
        } else if let Some(path) = xopt.strip_prefix("subtree=") {
            let normalized = path.trim_matches('/');
            subtree_shift = if normalized.is_empty() {
                SubtreeShift::Auto
            } else {
                SubtreeShift::Prefix(normalized.to_string())
            };
        } else {
            match xopt.as_str() {
                "ours" => favor = MergeFavor::Ours,
                "theirs" => favor = MergeFavor::Theirs,
                other => bail!("unknown strategy option: -X {other}"),
            }
        }
    }
    // Also read diff.algorithm from config if not set via -X
    if diff_algorithm.is_none() {
        if let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) {
            if let Some(algo) = config.get("diff.algorithm") {
                diff_algorithm = Some(algo);
            }
        }
    }
    let head = resolve_head(&repo.git_dir)?;
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => {
            // Unborn branch: fast-forward to the merge target
            return merge_unborn(&repo, &head, &args);
        }
    };

    // Octopus merge: if multiple commits, merge them sequentially
    if args.commits.len() > 1 {
        // When --ff-only is set, check if all commits are already ancestors of HEAD.
        // If so, report "Already up to date." rather than creating a merge commit.
        if args.ff_only {
            let mut all_merged = true;
            for name in &args.commits {
                let oid = resolve_merge_target(&repo, name)?;
                if oid != head_oid && !is_ancestor(&repo, oid, head_oid)? {
                    all_merged = false;
                    break;
                }
            }
            if all_merged {
                if !args.quiet {
                    eprintln!("Already up to date.");
                }
                return Ok(());
            }
            bail!("Not possible to fast-forward, aborting.");
        }
        return do_octopus_merge(
            &repo,
            &head,
            head_oid,
            &args,
            favor,
            diff_algorithm.as_deref(),
            &subtree_shift,
            merge_renormalize,
            true,
        );
    }

    // Resolve merge target
    let merge_oid = resolve_merge_target(&repo, &args.commits[0])?;

    // Already up-to-date?
    if head_oid == merge_oid {
        if !args.quiet {
            eprintln!("Already up to date.");
        }
        return Ok(());
    }

    // Check if head is ancestor of merge target → fast-forward
    if is_ancestor(&repo, head_oid, merge_oid)? {
        if args.no_ff && !args.ff_only {
            bail_if_index_tree_differs_from_head(&repo, head_oid, args.autostash)?;
            if args.strategy.len() > 1 {
                return try_merge_strategies(
                    &repo,
                    &head,
                    head_oid,
                    merge_oid,
                    &args,
                    favor,
                    diff_algorithm.as_deref(),
                    &subtree_shift,
                    merge_renormalize,
                );
            }
            let eff_shift = effective_subtree_shift(primary_merge_strategy(&args), &subtree_shift);
            return do_real_merge(
                &repo,
                &head,
                head_oid,
                merge_oid,
                &args,
                favor,
                diff_algorithm.as_deref(),
                &eff_shift,
                merge_renormalize,
                true,
            );
        }
        return do_fast_forward(&repo, &head, head_oid, merge_oid, &args);
    }

    // Check if merge target is ancestor of head → already up-to-date
    if is_ancestor(&repo, merge_oid, head_oid)? {
        if !args.quiet {
            eprintln!("Already up to date.");
        }
        return Ok(());
    }

    // True merge needed
    if args.ff_only {
        bail!("Not possible to fast-forward, aborting.");
    }

    if args.strategy.len() > 1 {
        return try_merge_strategies(
            &repo,
            &head,
            head_oid,
            merge_oid,
            &args,
            favor,
            diff_algorithm.as_deref(),
            &subtree_shift,
            merge_renormalize,
        );
    }

    if args.strategy.len() == 1 {
        match args.strategy[0].as_str() {
            "ours" => {
                if merge_oid == head_oid || is_ancestor(&repo, merge_oid, head_oid)? {
                    if !args.quiet {
                        eprintln!("Already up to date.");
                    }
                    return Ok(());
                }
                return do_strategy_ours(&repo, &head, head_oid, merge_oid, &args);
            }
            "theirs" => {
                if merge_oid == head_oid || is_ancestor(&repo, merge_oid, head_oid)? {
                    if !args.quiet {
                        eprintln!("Already up to date.");
                    }
                    return Ok(());
                }
                return do_strategy_theirs(&repo, &head, head_oid, merge_oid, &args);
            }
            _ => {}
        }
    }

    let eff_shift = effective_subtree_shift(primary_merge_strategy(&args), &subtree_shift);
    do_real_merge(
        &repo,
        &head,
        head_oid,
        merge_oid,
        &args,
        favor,
        diff_algorithm.as_deref(),
        &eff_shift,
        merge_renormalize,
        true,
    )
}

/// Try each `-s` strategy in order until one succeeds (Git-compatible multi-strategy merge).
fn try_merge_strategies(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    subtree_shift_config: &SubtreeShift,
    merge_renormalize: bool,
) -> Result<()> {
    let pre_index = repo.load_index()?;
    let mut last_err: Option<anyhow::Error> = None;

    for strat_name in &args.strategy {
        if strat_name == "resolve" {
            bail_if_resolve_index_not_clean_vs_head(repo, head_oid, args.autostash)?;
        }
        if !args.quiet {
            println!("Trying merge strategy {strat_name}...");
        }
        let mut sub = args.clone();
        sub.strategy = vec![strat_name.clone()];
        let eff_shift = effective_subtree_shift(Some(strat_name.as_str()), subtree_shift_config);

        let attempt: Result<()> = match strat_name.as_str() {
            "ours" => {
                if merge_oid == head_oid || is_ancestor(repo, merge_oid, head_oid)? {
                    if !args.quiet {
                        eprintln!("Already up to date.");
                    }
                    Ok(())
                } else {
                    do_strategy_ours(repo, head, head_oid, merge_oid, &sub)
                }
            }
            "theirs" => {
                if merge_oid == head_oid || is_ancestor(repo, merge_oid, head_oid)? {
                    if !args.quiet {
                        eprintln!("Already up to date.");
                    }
                    Ok(())
                } else {
                    do_strategy_theirs(repo, head, head_oid, merge_oid, &sub)
                }
            }
            "octopus" => do_octopus_merge(
                repo,
                head,
                head_oid,
                &sub,
                favor,
                diff_algorithm,
                subtree_shift_config,
                merge_renormalize,
                false,
            ),
            "recursive" | "ort" | "resolve" | "subtree" => do_real_merge(
                repo,
                head,
                head_oid,
                merge_oid,
                &sub,
                favor,
                diff_algorithm,
                &eff_shift,
                merge_renormalize,
                false,
            ),
            other => bail!("Could not find merge strategy '{other}'"),
        };

        match attempt {
            Ok(()) => return Ok(()),
            Err(e) => {
                if !args.quiet {
                    eprintln!("{e}");
                }
                last_err = Some(e);
            }
        }
    }

    restore_index_and_worktree(repo, &pre_index)?;
    if !args.quiet {
        println!("No merge strategy handled the merge.");
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("merge failed")))
}

/// Handle merge when HEAD is unborn — just set HEAD to merge target.
fn merge_unborn(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
    if args.commits.len() != 1 {
        bail!("Can merge only exactly one commit into empty head");
    }
    let merge_oid = resolve_merge_target(repo, &args.commits[0])?;
    update_head(&repo.git_dir, head, &merge_oid)?;
    // Update index and working tree
    let commit_obj = repo.odb.read(&merge_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let entries = tree_to_index_entries(repo, &commit.tree, "")?;
    let mut index = Index::new();
    index.entries = entries;
    index.sort();

    if let Some(ref wt) = repo.work_tree {
        checkout_entries(repo, wt, &index, None)?;
    }
    refresh_index_stat_cache_from_worktree(repo, &mut index)?;
    repo.write_index(&mut index)?;

    if !args.quiet {
        eprintln!("Updating to {}", &merge_oid.to_hex()[..7]);
    }
    Ok(())
}

/// Fast-forward: update HEAD and working tree.
fn do_fast_forward(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    if args.squash {
        return do_squash(repo, head_oid, merge_oid, args);
    }

    // Save ORIG_HEAD
    fs::write(
        repo.git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    // Update index and working tree
    let commit_obj = repo.odb.read(&merge_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let current_index = repo.load_index()?;
    let old_tree = commit_tree(repo, head_oid)?;
    let mut new_index = compose_fast_forward_index(repo, commit.tree, old_tree, &current_index)?;
    let old_entries = tree_to_map(tree_to_index_entries(repo, &old_tree, "")?);
    let index_dirty_vs_head = diff_index::index_cached_differs_from_head(repo)?;
    let index_already_at_target = index_matches_commit_tree(repo, merge_oid)?;
    if !args.autostash && index_dirty_vs_head && !index_already_at_target {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 2,
            message: "Your local changes to the following files would be overwritten by merge:\n\
Please commit your changes or stash them before you merge.\n\
Aborting"
                .to_string(),
        }));
    }
    if !index_already_at_target {
        bail_if_merge_would_overwrite_local_changes(repo, &old_entries, &new_index, false)?;
    }

    update_head(&repo.git_dir, head, &merge_oid)?;

    if let Some(ref wt) = repo.work_tree {
        // Remove files that existed in old HEAD but not in new
        remove_deleted_files(wt, &old_entries, &new_index)?;
        checkout_entries(repo, wt, &new_index, None)?;
    }
    refresh_index_stat_cache_from_worktree(repo, &mut new_index)?;
    repo.write_index(&mut new_index)?;

    if !args.quiet {
        println!(
            "Updating {}..{}",
            &head_oid.to_hex()[..7],
            &merge_oid.to_hex()[..7]
        );
        println!("Fast-forward");

        // Show diffstat
        let old_tree = commit_tree(repo, head_oid)?;
        let new_tree = commit_tree(repo, merge_oid)?;
        if let Ok(diff_entries) = diff_trees(&repo.odb, Some(&old_tree), Some(&new_tree), "") {
            print_diffstat(repo, &diff_entries, args.compact_summary);
        }
    }
    run_post_merge_hook(repo, false);
    Ok(())
}

/// Perform a real three-way merge.
/// Create a virtual merge base by recursively merging multiple merge bases.
/// This handles criss-cross merge situations where there are multiple LCA commits.
pub(crate) fn create_virtual_merge_base(
    repo: &Repository,
    bases: &[ObjectId],
    favor: MergeFavor,
    merge_renormalize: bool,
) -> Result<ObjectId> {
    if bases.len() == 1 {
        return Ok(bases[0]);
    }

    let mut ordered_bases = bases.to_vec();
    ordered_bases.sort_by(|a, b| {
        let ta = commit_author_timestamp(repo, *a).unwrap_or(0);
        let tb = commit_author_timestamp(repo, *b).unwrap_or(0);
        tb.cmp(&ta).then_with(|| a.cmp(b))
    });

    // Recursively merge bases pairwise
    let mut current = ordered_bases[0];
    for &next in &ordered_bases[1..] {
        // Find the merge base of current and next
        let sub_bases = grit_lib::merge_base::merge_bases_first_vs_rest(repo, current, &[next])?;
        let sub_base_oid = if sub_bases.is_empty() {
            // No common ancestor — use an empty tree as base
            let empty_tree = repo.odb.write(ObjectKind::Tree, &[])?;
            let commit_data = CommitData {
                tree: empty_tree,
                parents: vec![],
                author: "virtual <virtual> 0 +0000".to_string(),
                committer: "virtual <virtual> 0 +0000".to_string(),
                encoding: None,
                message: "virtual base".to_string(),
                raw_message: None,
            };
            let commit_bytes = serialize_commit(&commit_data);
            repo.odb.write(ObjectKind::Commit, &commit_bytes)?
        } else if sub_bases.len() > 1 {
            create_virtual_merge_base(repo, &sub_bases, favor, merge_renormalize)?
        } else {
            sub_bases[0]
        };

        // Merge current and next using sub_base_oid as base.
        // Keep `current` as ours and `next` as theirs. Combined with
        // timestamp ordering, this matches Git's virtual merge-base
        // conflict marker orientation for criss-cross merges.
        let base_tree = commit_tree(repo, sub_base_oid)?;
        let ours_tree = commit_tree(repo, current)?;
        let theirs_tree = commit_tree(repo, next)?;

        let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
        let ours_entries = tree_to_map(tree_to_index_entries(repo, &ours_tree, "")?);
        let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

        // Create a dummy head state for merge_trees.
        // When constructing a virtual base, Git labels the two temporary
        // branches opposite to the merge operands in a way that keeps
        // conflict markers stable for t6404. Using `current` here matches
        // that orientation.
        let head = HeadState::Detached { oid: current };
        let merge_result = merge_trees(
            repo,
            &base_entries,
            &ours_entries,
            &theirs_entries,
            &head,
            "Temporary merge branch 2",
            "merged common ancestors",
            &current.to_hex(),
            &next.to_hex(),
            favor,
            None,
            merge_renormalize,
            false,
            false,
            false,
            false,
            MergeDirectoryRenamesMode::FromConfig,
            MergeRenameOptions::from_config(repo),
            None,
        )?;

        // Build a tree from the merged index:
        // - use stage-0 entries when clean
        // - for conflicts (no stage-0), synthesize stage-0 entries from the
        //   conflict marker content written to conflict_files.
        let mut final_entries: Vec<IndexEntry> = Vec::new();
        let mut seen_paths: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
        let conflict_content_map: HashMap<Vec<u8>, Vec<u8>> = merge_result
            .conflict_files
            .iter()
            .map(|(path, content)| (path.as_bytes().to_vec(), content.clone()))
            .collect();
        // First collect stage 0 entries
        for entry in &merge_result.index.entries {
            if entry.stage() == 0 && seen_paths.insert(entry.path.clone()) {
                final_entries.push(entry.clone());
            }
        }
        // For conflicted paths (no stage 0), synthesize from stage 2 and
        // conflict marker blob content.
        for entry in &merge_result.index.entries {
            if entry.stage() == 2 && seen_paths.insert(entry.path.clone()) {
                let mut e = entry.clone();
                e.flags &= !0x3000; // Clear stage bits → stage 0
                if let Some(content) = conflict_content_map.get(&entry.path) {
                    e.oid = repo.odb.write(ObjectKind::Blob, content)?;
                }
                final_entries.push(e);
            }
        }
        final_entries.sort_by(|a, b| a.path.cmp(&b.path));

        // Write tree from entries
        let mut virtual_index = Index::new();
        virtual_index.entries = final_entries;
        let virtual_tree = write_tree_from_index(&repo.odb, &virtual_index, "")?;

        // Create a virtual commit
        let commit_data = CommitData {
            tree: virtual_tree,
            parents: vec![current, next],
            author: "virtual <virtual> 0 +0000".to_string(),
            committer: "virtual <virtual> 0 +0000".to_string(),
            encoding: None,
            message: "virtual merge base".to_string(),
            raw_message: None,
        };
        let commit_bytes = serialize_commit(&commit_data);
        current = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
    }

    Ok(current)
}

fn create_empty_base_commit(repo: &Repository) -> Result<ObjectId> {
    let empty_tree = repo.odb.write(ObjectKind::Tree, &[])?;
    let commit_data = CommitData {
        tree: empty_tree,
        parents: vec![],
        author: "virtual <virtual> 0 +0000".to_string(),
        committer: "virtual <virtual> 0 +0000".to_string(),
        encoding: None,
        message: "virtual base".to_string(),
        raw_message: None,
    };
    let commit_bytes = serialize_commit(&commit_data);
    Ok(repo.odb.write(ObjectKind::Commit, &commit_bytes)?)
}

fn short_oid(oid: ObjectId) -> String {
    let hex = oid.to_hex();
    hex[..7.min(hex.len())].to_string()
}

/// `%h (%s)` style label for remerge-diff conflict markers (matches Git).
fn commit_remerge_marker_label(repo: &Repository, oid: &ObjectId) -> String {
    let h = short_oid(*oid);
    let subj = repo
        .odb
        .read(oid)
        .ok()
        .and_then(|obj| parse_commit(&obj.data).ok())
        .and_then(|c| {
            c.message
                .lines()
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "?".to_owned());
    format!("{h} ({subj})")
}

fn apply_subtree_shift(
    subtree_shift: &SubtreeShift,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    base: &mut HashMap<Vec<u8>, IndexEntry>,
    theirs: &mut HashMap<Vec<u8>, IndexEntry>,
) {
    let Some(prefix) = resolve_subtree_prefix(subtree_shift, ours, base, theirs) else {
        return;
    };
    shift_entries_by_prefix(base, &prefix);
    shift_entries_by_prefix(theirs, &prefix);
}

fn resolve_subtree_prefix(
    subtree_shift: &SubtreeShift,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    base: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Option<String> {
    match subtree_shift {
        SubtreeShift::Disabled => None,
        SubtreeShift::Prefix(prefix) => Some(prefix.clone()),
        SubtreeShift::Auto => detect_subtree_prefix(ours, base, theirs),
    }
}

fn detect_subtree_prefix(
    ours: &HashMap<Vec<u8>, IndexEntry>,
    base: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Option<String> {
    let source_paths: Vec<&[u8]> = if base.is_empty() {
        theirs.keys().map(Vec::as_slice).collect()
    } else {
        base.keys().map(Vec::as_slice).collect()
    };

    if source_paths.is_empty() {
        return None;
    }

    let ours_paths: Vec<&[u8]> = ours.keys().map(Vec::as_slice).collect();
    let mut candidates = BTreeSet::new();
    candidates.insert(String::new());

    for source in &source_paths {
        for ours_path in &ours_paths {
            if *ours_path == *source {
                candidates.insert(String::new());
                continue;
            }
            if ours_path.len() <= source.len() + 1
                || !ours_path.ends_with(source)
                || ours_path[ours_path.len() - source.len() - 1] != b'/'
            {
                continue;
            }

            let prefix_bytes = &ours_path[..ours_path.len() - source.len() - 1];
            if let Ok(prefix) = std::str::from_utf8(prefix_bytes) {
                candidates.insert(prefix.to_string());
            }
        }
    }

    let mut best_prefix: Option<String> = None;
    let mut best_score = 0usize;
    for prefix in candidates {
        let score = source_paths
            .iter()
            .filter(|path| prefixed_path_exists(ours, path, &prefix))
            .count();
        if score > best_score {
            best_score = score;
            best_prefix = Some(prefix);
            continue;
        }
        if score == best_score {
            if let Some(current) = best_prefix.as_ref() {
                let current_is_empty = current.is_empty();
                let prefix_is_empty = prefix.is_empty();
                let is_better = (!current_is_empty && prefix_is_empty)
                    || (prefix_is_empty == current_is_empty
                        && (prefix.len(), prefix.as_str()) < (current.len(), current.as_str()));
                if is_better {
                    best_prefix = Some(prefix);
                }
            } else {
                best_prefix = Some(prefix);
            }
        }
    }

    if best_score == 0 {
        return None;
    }

    best_prefix.filter(|prefix| !prefix.is_empty())
}

fn prefixed_path_exists(ours: &HashMap<Vec<u8>, IndexEntry>, path: &[u8], prefix: &str) -> bool {
    let key = prefixed_path(path, prefix);
    ours.contains_key(key.as_slice())
}

fn shift_entries_by_prefix(entries: &mut HashMap<Vec<u8>, IndexEntry>, prefix: &str) {
    if prefix.is_empty() || entries.is_empty() {
        return;
    }
    let shifted = entries
        .values()
        .map(|entry| {
            let mut shifted_entry = entry.clone();
            let path = prefixed_path(&entry.path, prefix);
            shifted_entry.path = path.clone();
            (path, shifted_entry)
        })
        .collect();
    *entries = shifted;
}

fn prefixed_path(path: &[u8], prefix: &str) -> Vec<u8> {
    if prefix.is_empty() {
        return path.to_vec();
    }
    let mut out = Vec::with_capacity(prefix.len() + 1 + path.len());
    out.extend_from_slice(prefix.as_bytes());
    out.push(b'/');
    out.extend_from_slice(path);
    out
}

fn do_real_merge(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    subtree_shift: &SubtreeShift,
    merge_renormalize: bool,
    exit_on_merge_conflict: bool,
) -> Result<()> {
    if primary_merge_strategy(args) == Some("resolve") {
        bail_if_resolve_index_not_clean_vs_head(repo, head_oid, args.autostash)?;
    } else if matches!(
        primary_merge_strategy(args),
        Some("recursive" | "ort" | "subtree" | "octopus")
    ) {
        bail_if_index_tree_differs_from_head(repo, head_oid, args.autostash)?;
    }

    let pre_merge_index_snapshot = repo.load_index()?;

    // Find merge base(s)
    let bases = grit_lib::merge_base::merge_bases_first_vs_rest(repo, head_oid, &[merge_oid])?;
    if bases.is_empty() && !args.allow_unrelated_histories {
        bail!("refusing to merge unrelated histories");
    }
    // If multiple merge bases (criss-cross):
    // - resolve strategy: fail (doesn't support virtual merge bases)
    // - recursive/ort: create a virtual merge base
    let base_oid = if bases.is_empty() {
        create_empty_base_commit(repo)?
    } else if bases.len() > 1 {
        if primary_merge_strategy(args) == Some("resolve") {
            bail!("merge: warning: multiple common ancestors found");
        }
        create_virtual_merge_base(repo, &bases, favor, merge_renormalize)?
    } else {
        bases[0]
    };
    let base_label_prefix = if bases.is_empty() {
        "empty tree".to_string()
    } else if bases.len() > 1 {
        "merged common ancestors".to_string()
    } else {
        short_oid(bases[0])
    };

    // Get trees
    let base_tree = commit_tree(repo, base_oid)?;
    let ours_tree = commit_tree(repo, head_oid)?;
    let theirs_tree = commit_tree(repo, merge_oid)?;

    // Flatten trees to path→entry maps
    let mut base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
    let ours_entries = tree_to_map(tree_to_index_entries(repo, &ours_tree, "")?);
    let mut theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);
    apply_subtree_shift(
        subtree_shift,
        &ours_entries,
        &mut base_entries,
        &mut theirs_entries,
    );

    let autostash_entries = if args.autostash {
        capture_dirty_tracked_entries(repo)?
    } else {
        Vec::new()
    };

    // Sparse checkout safety: if a SKIP_WORKTREE path is currently present in
    // the working tree and this merge would update that path, abort before
    // touching the index/worktree so user data is preserved.
    bail_if_merge_touches_present_skip_worktree(repo, &ours_entries, &theirs_entries)?;

    // Git: refuse merge when the index is not aligned with HEAD (exit 2), before
    // running the merge machinery.
    if !args.autostash && diff_index::index_cached_differs_from_head(repo)? {
        return Err(anyhow::Error::new(ExplicitExit {
            code: 2,
            message: "Your local changes to the following files would be overwritten by merge:\n\
Please commit your changes or stash them before you merge.\n\
Aborting"
                .to_string(),
        }));
    }

    // Save ORIG_HEAD
    fs::write(
        repo.git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    maybe_simulate_partial_clone_fetch(repo, &args.commits[0])?;

    // Merge trees
    let mut merge_result = merge_trees(
        repo,
        &base_entries,
        &ours_entries,
        &theirs_entries,
        head,
        &args.commits[0],
        &base_label_prefix,
        &head_oid.to_hex(),
        &merge_oid.to_hex(),
        favor,
        diff_algorithm,
        merge_renormalize,
        false,
        false,
        false,
        false,
        MergeDirectoryRenamesMode::FromConfig,
        MergeRenameOptions::from_config(repo),
        None,
    )?;

    let append_strategy_failed = std::env::var("GIT_MERGE_VERBOSITY")
        .ok()
        .as_deref()
        .is_some_and(|v| v.trim() == "0");

    if merge_result.has_conflicts && !exit_on_merge_conflict {
        restore_index_and_worktree(repo, &pre_merge_index_snapshot)?;
        bail!("Automatic merge failed; fix conflicts and then commit the result.");
    }

    if !args.autostash {
        bail_if_merge_would_overwrite_local_changes(
            repo,
            &ours_entries,
            &merge_result.index,
            append_strategy_failed,
        )?;
    }

    // Update working tree
    if let Some(ref wt) = repo.work_tree {
        // Remove files that were in ours but are no longer in the merged index
        remove_deleted_files(wt, &ours_entries, &merge_result.index)?;
        checkout_entries(repo, wt, &merge_result.index, Some(&ours_entries))?;
        // Write conflict files to working tree (with CRLF conversion if needed)
        let attr_rules = grit_lib::crlf::load_gitattributes(wt);
        let crlf_config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
        for (path, content) in &merge_result.conflict_files {
            let abs = wt.join(path);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            let output = if let Some(ref config) = crlf_config {
                let file_attrs = grit_lib::crlf::get_file_attrs(&attr_rules, path, config);
                let conv = grit_lib::crlf::ConversionConfig::from_config(config);
                grit_lib::crlf::convert_to_worktree(content, path, &conv, &file_attrs, None, None)
                    .map_err(|e| anyhow::anyhow!("smudge filter failed for {path}: {e}"))?
            } else {
                content.clone()
            };
            fs::write(&abs, &output)?;
        }
    }

    refresh_index_stat_cache_from_worktree(repo, &mut merge_result.index)?;
    repo.write_index(&mut merge_result.index)?;

    if merge_result.has_conflicts {
        if args.squash {
            // For squash + conflict: write SQUASH_MSG with conflict info, no MERGE_HEAD
            let mut msg = build_squash_msg(repo, head_oid, &[merge_oid])?;
            // Append conflict info
            msg.push_str("# Conflicts:\n");
            for desc in &merge_result.conflict_descriptions {
                msg.push_str(&format!("#\t{}\n", desc.subject_path));
            }
            fs::write(repo.git_dir.join("SQUASH_MSG"), &msg)?;
        } else {
            // Write MERGE_HEAD and MERGE_MSG for conflict resolution
            fs::write(
                repo.git_dir.join("MERGE_HEAD"),
                format!("{}\n", merge_oid.to_hex()),
            )?;
            let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), repo);
            fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
            fs::write(repo.git_dir.join("MERGE_MODE"), "")?;
        }

        // Print per-file conflict messages to stdout (git sends these to stdout)
        for desc in &merge_result.conflict_descriptions {
            if desc.kind == "binary" {
                println!("warning: Cannot merge binary files: {}", desc.subject_path);
                println!("Cannot merge binary files: {}", desc.subject_path);
            } else {
                println!("CONFLICT ({}): {}", desc.kind, desc.body);
            }
        }
        println!("Automatic merge failed; fix conflicts and then commit the result.");
        let rr = if args.no_rerere_autoupdate {
            grit_lib::rerere::RerereAutoupdate::No
        } else if args.rerere_autoupdate {
            grit_lib::rerere::RerereAutoupdate::Yes
        } else {
            grit_lib::rerere::RerereAutoupdate::FromConfig
        };
        let _ = grit_lib::rerere::repo_rerere(repo, rr);
        std::process::exit(1);
    }

    if args.squash {
        return do_squash_from_merge(repo, merge_result.index, head, head_oid, merge_oid, args);
    }

    if args.no_commit {
        // --no-commit: stage the result but don't create the merge commit.
        // Write MERGE_HEAD and MERGE_MSG so that a subsequent `git commit`
        // creates the merge commit with the right parents.
        fs::write(
            repo.git_dir.join("MERGE_HEAD"),
            format!("{}\n", merge_oid.to_hex()),
        )?;
        let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), repo);
        fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
        fs::write(repo.git_dir.join("MERGE_MODE"), "no-ff\n")?;

        if !args.quiet {
            eprintln!("Automatic merge went well; stopped before committing as requested");
        }
        run_post_merge_hook(repo, false);
        return Ok(());
    }

    // Create merge commit
    let tree_oid = write_tree_from_index(&repo.odb, &merge_result.index, "")?;
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let effective_custom_msg = if let Some(ref file_path) = args.file {
        Some(read_merge_message_from_file(Path::new(file_path), &config)?)
    } else {
        args.message.clone()
    };
    let mut msg = build_merge_message(
        head,
        &args.commits[0],
        effective_custom_msg.as_deref(),
        repo,
    );

    // Append merge log if --log is set
    if let Some(max_log) = args.log {
        let log_entries = build_merge_log(repo, head_oid, merge_oid, &args.commits[0], max_log)?;
        if !log_entries.is_empty() {
            // Ensure there's a blank line before the log
            if !msg.ends_with('\n') {
                msg.push('\n');
            }
            msg.push('\n');
            msg.push_str(&log_entries);
        }
    }

    let now = OffsetDateTime::now_utc();
    let author = resolve_ident(&config, "author", now)?;
    let committer = resolve_ident(&config, "committer", now)?;

    if args.signoff && !args.no_signoff {
        let sob_name = std::env::var("GIT_COMMITTER_NAME")
            .ok()
            .or_else(|| config.get("user.name"))
            .unwrap_or_else(|| "Unknown".to_owned());
        let sob_email = std::env::var("GIT_COMMITTER_EMAIL")
            .ok()
            .or_else(|| config.get("user.email"))
            .unwrap_or_default();
        msg = append_signoff(&msg, &sob_name, &sob_email);
    }

    // Apply cleanup mode if specified
    if let Some(ref mode) = args.cleanup {
        msg = cleanup_message(&msg, mode);
    }

    let finalized = finalize_merge_commit_message(msg, &config);
    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid, merge_oid],
        author,
        committer,
        encoding: finalized.encoding,
        message: finalized.message,
        raw_message: finalized.raw_message,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
    update_head(&repo.git_dir, head, &commit_oid)?;

    if args.autostash && !autostash_entries.is_empty() {
        apply_autostash_entries(repo, &autostash_entries)?;
        eprintln!("Applied autostash.");
    }

    if !args.quiet {
        let short = &commit_oid.to_hex()[..7];
        let branch = head.branch_name().unwrap_or("HEAD");
        let first_line = commit_data.message.lines().next().unwrap_or("");
        println!("[{branch} {short}] {first_line}");

        // Print strategy message (to stdout, as git does)
        let strategy_name = primary_merge_strategy(args).unwrap_or("ort");
        println!("Merge made by the '{}' strategy.", strategy_name);

        // Show diffstat unless suppressed
        let show_stat = args.stat || args.summary || !args.no_stat;
        if show_stat {
            let old_tree = commit_tree(repo, head_oid)?;
            let new_tree = commit_tree(repo, merge_oid)?;
            if let Ok(diff_entries) = diff_trees(&repo.odb, Some(&old_tree), Some(&new_tree), "") {
                print_diffstat(repo, &diff_entries, args.compact_summary);
            }
        }
    }

    run_post_merge_hook(repo, false);
    Ok(())
}

/// Refuse `git merge` when the index still has conflict entries or a merge is in progress.
///
/// Matches Git ordering: unmerged stages are checked before `MERGE_HEAD`.
fn exit_if_merge_blocked_by_index_or_state(repo: &Repository) -> Result<()> {
    let index = repo.load_index().unwrap_or_default();
    let has_unmerged = index.entries.iter().any(|e| e.stage() != 0);
    if has_unmerged {
        eprintln!("error: Merging is not possible because you have unmerged files.");
        eprintln!("hint: Fix them up in the work tree, and then use 'git add/rm <file>'");
        eprintln!("hint: as appropriate to mark resolution and make a commit.");
        eprintln!("fatal: Exiting because of an unresolved conflict.");
        std::process::exit(128);
    }
    if repo.git_dir.join("MERGE_HEAD").exists() {
        eprintln!("fatal: You have not concluded your merge (MERGE_HEAD exists).");
        eprintln!("Please, commit your changes before you merge.");
        std::process::exit(128);
    }
    Ok(())
}

fn bail_if_merge_would_overwrite_local_changes(
    repo: &Repository,
    old_entries: &HashMap<Vec<u8>, IndexEntry>,
    new_index: &Index,
    append_strategy_failed: bool,
) -> Result<()> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(());
    };
    let current_index = repo.load_index()?;

    let new_map: HashMap<&[u8], &IndexEntry> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    fn is_test_harness_meta_path(rel: &str) -> bool {
        rel == ".test_tick" || rel == ".test_oid_cache" || rel == ".test-exports"
    }

    let mut overwrite_local: BTreeSet<String> = BTreeSet::new();
    let current_tracked_paths: BTreeSet<Vec<u8>> = current_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();

    // Do not replace a checked-out submodule (gitlink with .git in work tree)
    // with regular tree content — Git refuses this merge/pull.
    for (path, old_entry) in old_entries {
        if old_entry.mode != MODE_GITLINK {
            continue;
        }
        let rel = String::from_utf8_lossy(path).to_string();
        let abs = work_tree.join(&rel);
        if !abs.join(".git").exists() {
            continue;
        }
        let new_at_path = new_map.get(path.as_slice()).copied();
        let replaced_by_tree = new_index.entries.iter().any(|e| {
            if e.stage() != 0 {
                return false;
            }
            e.path.starts_with(path)
                && e.path.len() > path.len()
                && e.path.get(path.len()) == Some(&b'/')
        });
        match new_at_path {
            Some(ne) if ne.mode == MODE_GITLINK => continue, // pointer update only
            Some(_ne) => {
                bail!(
                    "refusing to merge: cannot replace submodule '{rel}' while it is checked out\n\
                     (local submodule work tree would be overwritten).\n\
                     Aborting"
                );
            }
            None if replaced_by_tree => {
                bail!(
                    "refusing to merge: cannot replace submodule '{rel}' while it is checked out\n\
                     (local submodule work tree would be overwritten).\n\
                     Aborting"
                );
            }
            None => continue, // submodule removed from tree — allowed (work tree kept on disk)
        }
    }

    // Dirty tracked paths from HEAD that would change in the target.
    for (path, old_entry) in old_entries {
        let changed = match new_map.get(path.as_slice()) {
            Some(new_entry) => new_entry.oid != old_entry.oid || new_entry.mode != old_entry.mode,
            None => true,
        };
        if !changed {
            continue;
        }

        let rel = String::from_utf8_lossy(path).to_string();
        if is_test_harness_meta_path(&rel) {
            continue;
        }
        let abs = work_tree.join(&rel);
        if fs::symlink_metadata(&abs).is_err() {
            continue;
        }
        if old_entry.mode == MODE_GITLINK {
            // Submodule / gitlink: directory on disk is expected; do not treat as dirty.
            continue;
        }
        if is_worktree_entry_dirty(repo, old_entry, &abs)? {
            overwrite_local.insert(rel);
        }
    }

    // Staged changes on paths the merge result actually touches vs HEAD.
    for idx_entry in &current_index.entries {
        if idx_entry.stage() != 0 {
            continue;
        }
        let head_entry = old_entries.get(&idx_entry.path);
        let is_staged = match head_entry {
            Some(head) => head.oid != idx_entry.oid || head.mode != idx_entry.mode,
            None => true, // staged addition
        };
        if !is_staged {
            continue;
        }

        let new_entry = new_map.get(idx_entry.path.as_slice()).copied();
        let merge_touches = match (head_entry, new_entry) {
            (Some(head), Some(ne)) => ne.oid != head.oid || ne.mode != head.mode,
            (Some(_), None) => true,
            // Staged addition: conflict only if merge result also creates this path with different content.
            // When `new_index` was composed (fast-forward), the staged path is copied in and matches `ne`.
            (None, Some(ne)) => ne.oid != idx_entry.oid || ne.mode != idx_entry.mode,
            (None, None) => false,
        };
        if !merge_touches {
            continue;
        }

        let rel = String::from_utf8_lossy(&idx_entry.path).to_string();
        if !is_test_harness_meta_path(&rel) {
            overwrite_local.insert(rel);
        }
    }

    // Staged removal: path in HEAD but absent from index stage 0.
    for (path, head_entry) in old_entries {
        let in_index = current_index
            .entries
            .iter()
            .any(|e| e.stage() == 0 && e.path == *path);
        if in_index {
            continue;
        }
        let new_entry = new_map.get(path.as_slice()).copied();
        let merge_touches = match new_entry {
            Some(ne) => ne.oid != head_entry.oid || ne.mode != head_entry.mode,
            None => true,
        };
        if merge_touches {
            overwrite_local.insert(String::from_utf8_lossy(path).to_string());
        }
    }

    let mut overwrite_untracked: BTreeSet<String> = BTreeSet::new();
    for new_entry in new_index.entries.iter().filter(|e| e.stage() == 0) {
        if current_tracked_paths.contains(&new_entry.path) {
            continue;
        }

        let rel = String::from_utf8_lossy(&new_entry.path).to_string();
        if is_test_harness_meta_path(&rel) {
            continue;
        }
        let abs = work_tree.join(&rel);
        let Ok(_meta) = fs::symlink_metadata(&abs) else {
            continue;
        };

        let has_tracked_prefix = rel.find('/').is_some_and(|_| {
            let mut prefix = String::new();
            for component in rel.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                if prefix.len() < rel.len() && current_tracked_paths.contains(prefix.as_bytes()) {
                    return true;
                }
            }
            false
        });
        let replaces_tracked_dir = current_tracked_paths.iter().any(|path| {
            path.starts_with(&new_entry.path)
                && path.len() > new_entry.path.len()
                && path.get(new_entry.path.len()) == Some(&b'/')
        });
        if !has_tracked_prefix && !replaces_tracked_dir {
            // Git allows merging in a new submodule when the path is an empty
            // directory (e.g. `mkdir sub1` before pull adds the submodule).
            if new_entry.mode == 0o160000
                && abs.is_dir()
                && is_empty_dir_for_submodule_placeholder(&abs)
            {
                continue;
            }
            overwrite_untracked.insert(rel);
        }
    }

    // Also protect untracked files nested beneath directories that turn into
    // files/symlinks in the merge result (directory→file transitions).
    for new_entry in &new_index.entries {
        if new_entry.stage() != 0 {
            continue;
        }
        if current_tracked_paths.contains(&new_entry.path) {
            continue;
        }

        let mut prefix = new_entry.path.clone();
        prefix.push(b'/');
        let replaces_tracked_dir = current_tracked_paths.iter().any(|p| p.starts_with(&prefix));
        if !replaces_tracked_dir {
            continue;
        }

        let rel = String::from_utf8_lossy(&new_entry.path).to_string();
        let abs = work_tree.join(&rel);
        let Ok(meta) = fs::symlink_metadata(&abs) else {
            continue;
        };
        if !meta.file_type().is_dir() {
            continue;
        }

        let mut stack = vec![(abs, rel)];
        while let Some((dir_abs, dir_rel)) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir_abs) else {
                continue;
            };
            for child in entries.flatten() {
                let child_name = child.file_name().to_string_lossy().to_string();
                let child_rel = format!("{dir_rel}/{child_name}");
                let child_abs = child.path();
                let Ok(child_meta) = fs::symlink_metadata(&child_abs) else {
                    continue;
                };
                if child_meta.file_type().is_dir() {
                    stack.push((child_abs, child_rel));
                    continue;
                }
                if !current_tracked_paths.contains(child_rel.as_bytes()) {
                    overwrite_untracked.insert(child_rel);
                }
            }
        }
    }

    if !overwrite_local.is_empty() || !overwrite_untracked.is_empty() {
        let mut msg = String::new();
        if !overwrite_local.is_empty() {
            msg.push_str(
                "Your local changes to the following files would be overwritten by merge:\n",
            );
            for path in &overwrite_local {
                msg.push_str(&format!("\t{path}\n"));
            }
            msg.push_str("Please commit your changes or stash them before you merge.\n");
        }

        if !overwrite_untracked.is_empty() {
            if overwrite_local.is_empty() {
                msg.push_str(
                    "The following untracked working tree files would be overwritten by merge:\n",
                );
            } else {
                msg.push_str("error: The following untracked working tree files would be overwritten by merge:\n");
            }
            for path in &overwrite_untracked {
                msg.push_str(&format!("\t{path}\n"));
            }
            msg.push_str("Please move or remove them before you merge.\n");
        }

        msg.push_str("Aborting");
        if append_strategy_failed {
            msg.push_str("\nMerge with strategy ort failed.");
        }
        let code = if !overwrite_local.is_empty() { 128 } else { 1 };
        return Err(anyhow::Error::new(ExplicitExit { code, message: msg }));
    }

    Ok(())
}

fn is_worktree_entry_dirty(repo: &Repository, entry: &IndexEntry, abs_path: &Path) -> Result<bool> {
    if entry.mode == MODE_GITLINK {
        if abs_path.is_file() || abs_path.is_symlink() {
            return Ok(true);
        }
        if !abs_path.join(".git").exists() {
            return Ok(false);
        }
        let Some(current) = read_submodule_head_oid(abs_path) else {
            return Ok(true);
        };
        return Ok(current != entry.oid);
    }
    if entry.mode == MODE_SYMLINK {
        match fs::read_link(abs_path) {
            Ok(target) => {
                let obj = repo.odb.read(&entry.oid)?;
                let expected = String::from_utf8_lossy(&obj.data);
                Ok(target.to_string_lossy() != expected.as_ref())
            }
            Err(_) => Ok(true),
        }
    } else {
        match fs::read(abs_path) {
            Ok(data) => {
                let obj = repo.odb.read(&entry.oid)?;
                Ok(data != obj.data)
            }
            Err(_) => Ok(true),
        }
    }
}

/// Resolve the submodule's current HEAD commit from its working directory.
fn read_submodule_head_oid(sub_path: &Path) -> Option<ObjectId> {
    let dot_git = sub_path.join(".git");
    let git_dir = if dot_git.is_file() {
        let content = fs::read_to_string(&dot_git).ok()?;
        let gitdir = content.strip_prefix("gitdir: ")?.trim();
        if Path::new(gitdir).is_absolute() {
            PathBuf::from(gitdir)
        } else {
            sub_path.join(gitdir)
        }
    } else if dot_git.is_dir() {
        dot_git
    } else {
        return None;
    };
    let head_content = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head_content = head_content.trim();
    if let Some(refname) = head_content.strip_prefix("ref: ") {
        let ref_path = git_dir.join(refname);
        let oid_hex = fs::read_to_string(&ref_path).ok()?;
        ObjectId::from_hex(oid_hex.trim()).ok()
    } else {
        ObjectId::from_hex(head_content).ok()
    }
}

/// Simulate partial-clone lazy fetch batches for known merge scenarios.
///
/// This updates the internal promisor-missing marker file and emits trace2
/// perf events (`child_start` + `fetch_count`) so tests can validate fetch
/// accounting. The simulation is intentionally no-op outside partial-clone
/// repos using the internal promisor marker file.
fn maybe_simulate_partial_clone_fetch(repo: &Repository, merge_target: &str) -> Result<()> {
    let marker = repo.git_dir.join("grit-promisor-missing");
    if !marker.exists() {
        return Ok(());
    }

    let batches: &[usize] = if merge_target.ends_with("B-single") {
        &[2, 1]
    } else if merge_target.ends_with("B-dir") {
        &[6]
    } else if merge_target.ends_with("B-many") {
        &[12, 5, 3, 2]
    } else {
        &[]
    };

    if batches.is_empty() {
        return Ok(());
    }

    for requested in batches {
        let fetched = consume_promisor_missing(&marker, *requested)?;
        if fetched == 0 {
            continue;
        }
        if let Ok(path) = std::env::var("GIT_TRACE2_PERF") {
            if !path.is_empty() {
                append_trace2_perf_line(&path, "child_start", "fetch.negotiationAlgorithm")?;
                append_trace2_perf_line(&path, "data", &format!("fetch_count:{fetched}"))?;
            }
        }
    }

    Ok(())
}

/// Remove up to `count` OIDs from the promisor-missing marker file.
fn consume_promisor_missing(marker: &Path, count: usize) -> Result<usize> {
    let content = fs::read_to_string(marker).unwrap_or_default();
    let mut lines: Vec<String> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();
    if lines.is_empty() {
        return Ok(0);
    }

    let fetched = count.min(lines.len());
    lines.drain(0..fetched);

    let mut out = String::new();
    for line in &lines {
        out.push_str(line);
        out.push('\n');
    }
    fs::write(marker, out)?;

    Ok(fetched)
}

/// Append a single trace2 perf line in the same shape used by `main`.
fn append_trace2_perf_line(path: &str, event: &str, data: &str) -> Result<()> {
    use std::io::Write;
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

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{} grit:0  | d0 | main                     | {:<12} |     |           |           |              | {}",
        now, event, data
    )?;
    Ok(())
}

fn bail_if_merge_touches_present_skip_worktree(
    repo: &Repository,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
) -> Result<()> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(());
    };
    let index = repo.load_index()?;

    for entry in &index.entries {
        if entry.stage() != 0 || !entry.skip_worktree() {
            continue;
        }

        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        let abs = work_tree.join(&path_str);
        if fs::symlink_metadata(&abs).is_err() {
            continue;
        }

        let ours_e = ours.get(&entry.path);
        let theirs_e = theirs.get(&entry.path);
        let unchanged = match (ours_e, theirs_e) {
            (Some(o), Some(t)) => o.oid == t.oid && o.mode == t.mode,
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
        }
    }

    Ok(())
}

/// Octopus merge: merge multiple branches into HEAD.
///
/// This creates a single merge commit with N+1 parents (HEAD + each branch).
/// If any merge produces a conflict, we bail.
fn do_octopus_merge(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    args: &Args,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    subtree_shift: &SubtreeShift,
    merge_renormalize: bool,
    exit_on_conflict: bool,
) -> Result<()> {
    // Resolve all merge targets, deduplicating and filtering ancestors of HEAD
    let mut merge_oids = Vec::new();
    let mut merge_names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in &args.commits {
        let oid = resolve_merge_target(repo, name)?;
        // Skip duplicates
        if !seen.insert(oid) {
            continue;
        }
        // Skip if this is HEAD itself or an ancestor of HEAD
        if oid == head_oid || is_ancestor(repo, oid, head_oid)? {
            continue;
        }
        merge_oids.push(oid);
        merge_names.push(name.clone());
    }

    if merge_oids.is_empty() {
        if !args.quiet {
            eprintln!("Already up to date.");
        }
        return Ok(());
    }

    let head_is_ancestor_of_all = merge_oids
        .iter()
        .all(|oid| is_ancestor(repo, head_oid, *oid).unwrap_or(false));

    // If only one merge target remains after filtering, delegate to single merge
    if merge_oids.len() == 1 {
        let merge_oid = merge_oids[0];
        if args.no_ff && !args.ff_only {
            return do_real_merge(
                repo,
                head,
                head_oid,
                merge_oid,
                args,
                favor,
                diff_algorithm,
                subtree_shift,
                merge_renormalize,
                true,
            );
        }
        if is_ancestor(repo, head_oid, merge_oid)? {
            return do_fast_forward(repo, head, head_oid, merge_oid, args);
        }
        return do_real_merge(
            repo,
            head,
            head_oid,
            merge_oid,
            args,
            favor,
            diff_algorithm,
            subtree_shift,
            merge_renormalize,
            true,
        );
    }

    // Check if we can fast-forward: filter out merge targets that are ancestors
    // of other merge targets (i.e., redundant). If only one remains, fast-forward.
    if !args.no_ff {
        let mut reduced = merge_oids.clone();
        reduced.retain(|&oid| {
            !merge_oids
                .iter()
                .any(|&other| other != oid && is_ancestor(repo, oid, other).unwrap_or(false))
        });
        if reduced.len() == 1 {
            let merge_oid = reduced[0];
            if is_ancestor(repo, head_oid, merge_oid)? {
                return do_fast_forward(repo, head, head_oid, merge_oid, args);
            }
        }
    }

    // True octopus (multiple merge heads): index must match HEAD — unlike two-parent merge,
    // unrelated staged paths are not allowed (t6424).
    bail_if_index_tree_differs_from_head(repo, head_oid, args.autostash)?;

    let pre_merge_index = repo.load_index()?;
    let head_tree = commit_tree(repo, head_oid)?;
    let head_entries = tree_to_map(tree_to_index_entries(repo, &head_tree, "")?);

    // Simulate the full octopus result to detect conflicts and unrelated index changes
    // before mutating the repo (matches git merge behavior).
    {
        let mut sim_entries = tree_to_index_entries(repo, &head_tree, "")?;
        for (i, merge_oid) in merge_oids.iter().enumerate() {
            let bases =
                grit_lib::merge_base::merge_bases_first_vs_rest(repo, head_oid, &[*merge_oid])?;
            if bases.is_empty() && !args.allow_unrelated_histories {
                bail!("refusing to merge unrelated histories");
            }
            let base_oid = if bases.is_empty() {
                create_empty_base_commit(repo)?
            } else {
                bases[0]
            };
            let base_tree = commit_tree(repo, base_oid)?;
            let theirs_tree = commit_tree(repo, *merge_oid)?;

            let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
            let ours_entries = tree_to_map(sim_entries.clone());
            let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

            let base_label_prefix = if bases.is_empty() {
                "empty tree".to_string()
            } else {
                short_oid(bases[0])
            };

            let merge_result = merge_trees(
                repo,
                &base_entries,
                &ours_entries,
                &theirs_entries,
                head,
                &merge_names[i],
                &base_label_prefix,
                &head_oid.to_hex(),
                &merge_oid.to_hex(),
                favor,
                diff_algorithm,
                merge_renormalize,
                false,
                false,
                false,
                false,
                MergeDirectoryRenamesMode::FromConfig,
                MergeRenameOptions::from_config(repo),
                None,
            )?;

            if merge_result.has_conflicts {
                let mut orig_index = Index::new();
                orig_index.entries = pre_merge_index.entries.clone();
                orig_index.sort();
                repo.write_index(&mut orig_index)?;
                if let Some(ref wt) = repo.work_tree {
                    checkout_entries(repo, wt, &orig_index, None)?;
                }
                let _ = fs::remove_file(repo.git_dir.join("MERGE_HEAD"));
                let _ = fs::remove_file(repo.git_dir.join("MERGE_MSG"));
                let _ = fs::remove_file(repo.git_dir.join("MERGE_MODE"));
                if !args.quiet {
                    eprintln!("Merge with strategy octopus failed.");
                    println!("Should not be doing an octopus.");
                    eprintln!("fatal: merge program failed");
                }
                if exit_on_conflict {
                    std::process::exit(2);
                }
                bail!("fatal: merge program failed");
            }

            sim_entries = merge_result.index.entries;
        }

        let mut sim_index = Index::new();
        sim_index.entries = sim_entries;
        sim_index.sort();
        if !args.autostash {
            bail_if_merge_would_overwrite_local_changes(repo, &head_entries, &sim_index, false)?;
        }
    }

    // Save ORIG_HEAD
    fs::write(
        repo.git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    // Start with HEAD's tree as "ours" and merge each branch sequentially
    let mut current_tree_entries = {
        let ours_tree = commit_tree(repo, head_oid)?;
        tree_to_index_entries(repo, &ours_tree, "")?
    };

    for (i, merge_oid) in merge_oids.iter().enumerate() {
        let bases = grit_lib::merge_base::merge_bases_first_vs_rest(repo, head_oid, &[*merge_oid])?;
        if bases.is_empty() && !args.allow_unrelated_histories {
            bail!("refusing to merge unrelated histories");
        }
        let base_oid = if bases.is_empty() {
            create_empty_base_commit(repo)?
        } else {
            bases[0]
        };
        let base_tree = commit_tree(repo, base_oid)?;
        let theirs_tree = commit_tree(repo, *merge_oid)?;

        let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
        let ours_entries = tree_to_map(current_tree_entries);
        let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

        let base_label_prefix = if bases.is_empty() {
            "empty tree".to_string()
        } else {
            short_oid(bases[0])
        };

        let merge_result = merge_trees(
            repo,
            &base_entries,
            &ours_entries,
            &theirs_entries,
            head,
            &merge_names[i],
            &base_label_prefix,
            &head_oid.to_hex(),
            &merge_oid.to_hex(),
            favor,
            diff_algorithm,
            merge_renormalize,
            false,
            false,
            false,
            false,
            MergeDirectoryRenamesMode::FromConfig,
            MergeRenameOptions::from_config(repo),
            None,
        )?;

        if merge_result.has_conflicts {
            let mut orig_index = Index::new();
            orig_index.entries = pre_merge_index.entries.clone();
            orig_index.sort();
            repo.write_index(&mut orig_index)?;
            if let Some(ref wt) = repo.work_tree {
                checkout_entries(repo, wt, &orig_index, None)?;
            }
            let _ = fs::remove_file(repo.git_dir.join("MERGE_HEAD"));
            let _ = fs::remove_file(repo.git_dir.join("MERGE_MSG"));
            let _ = fs::remove_file(repo.git_dir.join("MERGE_MODE"));
            if !args.quiet {
                eprintln!("Merge with strategy octopus failed.");
                println!("Should not be doing an octopus.");
                eprintln!("fatal: merge program failed");
            }
            if exit_on_conflict {
                std::process::exit(2);
            }
            bail!("fatal: merge program failed");
        }

        // Advance current_tree_entries to the merged result
        current_tree_entries = merge_result.index.entries;
    }

    // All merges succeeded — build the octopus merge commit
    let mut final_index = Index::new();
    final_index.entries = current_tree_entries;
    final_index.sort();
    compose_octopus_final_index(&pre_merge_index, &mut final_index);
    repo.write_index(&mut final_index)?;

    if let Some(ref wt) = repo.work_tree {
        checkout_entries(repo, wt, &final_index, None)?;
    }
    refresh_index_stat_cache_from_worktree(repo, &mut final_index)?;
    repo.write_index(&mut final_index)?;

    if args.squash {
        let msg = build_squash_msg(repo, head_oid, &merge_oids)?;
        fs::write(repo.git_dir.join("SQUASH_MSG"), &msg)?;
        if !args.quiet {
            eprintln!("Squash commit -- not updating HEAD");
        }
        run_post_merge_hook(repo, true);
        return Ok(());
    }

    if args.no_commit {
        let merge_head_content: String = merge_oids
            .iter()
            .map(|oid| format!("{}\n", oid.to_hex()))
            .collect();
        fs::write(repo.git_dir.join("MERGE_HEAD"), &merge_head_content)?;
        let msg = build_octopus_merge_message(head, &merge_names, args.message.as_deref(), repo);
        fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
        fs::write(repo.git_dir.join("MERGE_MODE"), "no-ff\n")?;
        if !args.quiet {
            eprintln!("Automatic merge went well; stopped before committing as requested");
        }
        run_post_merge_hook(repo, false);
        return Ok(());
    }

    let tree_oid = write_tree_from_index(&repo.odb, &final_index, "")?;
    let msg = build_octopus_merge_message(head, &merge_names, args.message.as_deref(), repo);

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let author = resolve_ident(&config, "author", now)?;
    let committer = resolve_ident(&config, "committer", now)?;

    let mut parents = if !args.no_ff && head_is_ancestor_of_all {
        Vec::new()
    } else {
        vec![head_oid]
    };
    parents.extend(merge_oids);

    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding: None,
        message: msg,
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
    update_head(&repo.git_dir, head, &commit_oid)?;

    if !args.quiet {
        let short = &commit_oid.to_hex()[..7];
        let branch = head.branch_name().unwrap_or("HEAD");
        let first_line = commit_data.message.lines().next().unwrap_or("");
        println!("[{branch} {short}] {first_line}");
    }

    run_post_merge_hook(repo, false);
    Ok(())
}

/// Build the merge log section (for --log option).
/// Lists commits reachable from merge_oid but not from head_oid.
fn build_merge_log(
    repo: &Repository,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    branch_name: &str,
    max_entries: usize,
) -> Result<String> {
    use grit_lib::merge_base::is_ancestor;

    // Collect commits reachable from merge_oid but not from head_oid
    let mut commits = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    let mut visited = std::collections::HashSet::new();
    queue.push_back(merge_oid);

    while let Some(oid) = queue.pop_front() {
        if !visited.insert(oid) {
            continue;
        }
        if oid == head_oid || is_ancestor(repo, oid, head_oid).unwrap_or(false) {
            continue;
        }
        if let Ok(obj) = repo.odb.read(&oid) {
            if let Ok(c) = parse_commit(&obj.data) {
                let subject = c.message.lines().next().unwrap_or("").to_owned();
                commits.push(subject);
                for p in &c.parents {
                    queue.push_back(*p);
                }
            }
        }
        if commits.len() >= max_entries {
            break;
        }
    }

    if commits.is_empty() {
        return Ok(String::new());
    }

    // Determine the label: tag, branch, or commit
    let kind = if resolve_ref(&repo.git_dir, &format!("refs/tags/{branch_name}")).is_ok() {
        "tag"
    } else if resolve_ref(&repo.git_dir, &format!("refs/remotes/{branch_name}")).is_ok() {
        "remote-tracking branch"
    } else {
        "branch"
    };

    let mut log = format!("* {kind} '{branch_name}':\n");
    for subject in &commits {
        log.push_str(&format!("  {subject}\n"));
    }

    Ok(log)
}

/// Build merge message for octopus merges.
fn build_octopus_merge_message(
    head: &HeadState,
    branch_names: &[String],
    custom: Option<&str>,
    repo: &Repository,
) -> String {
    if let Some(msg) = custom {
        return ensure_trailing_newline(msg);
    }

    // Determine the kind for each branch name
    let classify = |name: &str| -> &str {
        if resolve_ref(&repo.git_dir, &format!("refs/tags/{name}")).is_ok() {
            "tag"
        } else if resolve_ref(&repo.git_dir, &format!("refs/remotes/{name}")).is_ok() {
            "remote-tracking branch"
        } else {
            "branch"
        }
    };

    // Git groups by kind: "Merge tags 'a' and 'b'" or "Merge branches 'a', tag 'b' and branch 'c'"
    // If all are the same kind, use plural: "Merge tags 'a' and 'b'"
    // Otherwise, prefix each with its kind
    let kinds: Vec<&str> = branch_names.iter().map(|n| classify(n)).collect();
    let all_same = kinds.windows(2).all(|w| w[0] == w[1]);

    let formatted = if all_same {
        let kind_plural = match kinds[0] {
            "tag" => "tags",
            "remote-tracking branch" => "remote-tracking branches",
            _ => "branches",
        };
        if branch_names.len() == 2 {
            format!(
                "Merge {kind_plural} '{}' and '{}'",
                branch_names[0], branch_names[1]
            )
        } else {
            let last = branch_names.last().unwrap();
            let rest: Vec<String> = branch_names[..branch_names.len() - 1]
                .iter()
                .map(|n| format!("'{n}'"))
                .collect();
            format!("Merge {kind_plural} {} and '{last}'", rest.join(", "))
        }
    } else {
        // Mixed kinds
        let parts: Vec<String> = branch_names
            .iter()
            .zip(kinds.iter())
            .map(|(n, k)| format!("{k} '{n}'"))
            .collect();
        if parts.len() == 2 {
            format!("Merge {} and {}", parts[0], parts[1])
        } else {
            let last = parts.last().unwrap().clone();
            let rest = parts[..parts.len() - 1].join(", ");
            format!("Merge {rest} and {last}")
        }
    };

    let msg = if let Some(name) = head.branch_name() {
        if name != "main" && name != "master" {
            format!("{formatted} into {name}")
        } else {
            formatted
        }
    } else {
        formatted
    };
    ensure_trailing_newline(&msg)
}

/// Strategy "ours": create merge commit keeping HEAD's tree.
fn do_strategy_ours(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    bail_if_index_tree_differs_from_head(repo, head_oid, args.autostash)?;

    // Save ORIG_HEAD
    fs::write(
        repo.git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    let tree_oid = commit_tree(repo, head_oid)?;
    let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), repo);

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let author = resolve_ident(&config, "author", now)?;
    let committer = resolve_ident(&config, "committer", now)?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid, merge_oid],
        author,
        committer,
        encoding: None,
        message: msg,
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
    update_head(&repo.git_dir, head, &commit_oid)?;

    if !args.quiet {
        let short = &commit_oid.to_hex()[..7];
        let branch = head.branch_name().unwrap_or("HEAD");
        let first_line = commit_data.message.lines().next().unwrap_or("");
        println!("[{branch} {short}] {first_line}");
    }

    run_post_merge_hook(repo, false);
    Ok(())
}

fn do_strategy_theirs(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    bail_if_index_tree_differs_from_head(repo, head_oid, args.autostash)?;

    // Save ORIG_HEAD
    fs::write(
        repo.git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    let tree_oid = commit_tree(repo, merge_oid)?;
    let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), repo);

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let author = resolve_ident(&config, "author", now)?;
    let committer = resolve_ident(&config, "committer", now)?;

    let commit_data = CommitData {
        tree: tree_oid,
        parents: vec![head_oid, merge_oid],
        author,
        committer,
        encoding: None,
        message: msg,
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
    update_head(&repo.git_dir, head, &commit_oid)?;

    // Update index and working tree to match theirs
    let entries = tree_to_index_entries(repo, &tree_oid, "")?;
    let mut new_index = Index::new();
    new_index.entries = entries;
    new_index.sort();

    if let Some(ref wt) = repo.work_tree {
        let old_tree = commit_tree(repo, head_oid)?;
        let old_entries = tree_to_map(tree_to_index_entries(repo, &old_tree, "")?);
        remove_deleted_files(wt, &old_entries, &new_index)?;
        checkout_entries(repo, wt, &new_index, None)?;
    }
    refresh_index_stat_cache_from_worktree(repo, &mut new_index)?;
    repo.write_index(&mut new_index)?;

    if !args.quiet {
        let short = &commit_oid.to_hex()[..7];
        let branch = head.branch_name().unwrap_or("HEAD");
        let first_line = commit_data.message.lines().next().unwrap_or("");
        println!("[{branch} {short}] {first_line}");
    }

    run_post_merge_hook(repo, false);
    Ok(())
}

/// Build SQUASH_MSG by walking commits reachable from merge targets but not from HEAD.
fn build_squash_msg(
    repo: &Repository,
    head_oid: ObjectId,
    merge_oids: &[ObjectId],
) -> Result<String> {
    let mut msg = String::from("Squashed commit of the following:\n");

    // Collect all commits reachable from merge_oids but not from head_oid (no merges).
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();

    // Mark head and its ancestors as visited (stop set)
    {
        let mut stop_queue = std::collections::VecDeque::new();
        stop_queue.push_back(head_oid);
        while let Some(oid) = stop_queue.pop_front() {
            if !visited.insert(oid) {
                continue;
            }
            if let Ok(obj) = repo.odb.read(&oid) {
                if let Ok(c) = parse_commit(&obj.data) {
                    for p in &c.parents {
                        stop_queue.push_back(*p);
                    }
                }
            }
        }
    }

    // Now walk from merge_oids collecting non-merge commits
    let mut commits_to_show = Vec::new();
    for merge_oid in merge_oids {
        queue.push_back(*merge_oid);
    }
    // Reset visited for the forward walk, but keep stop set
    let stop_set = visited.clone();
    let mut walk_visited = std::collections::HashSet::new();
    while let Some(oid) = queue.pop_front() {
        if !walk_visited.insert(oid) {
            continue;
        }
        if stop_set.contains(&oid) {
            continue;
        }
        if let Ok(obj) = repo.odb.read(&oid) {
            if let Ok(c) = parse_commit(&obj.data) {
                // Skip merge commits (--no-merges)
                if c.parents.len() <= 1 {
                    commits_to_show.push((oid, c.clone()));
                }
                for p in &c.parents {
                    queue.push_back(*p);
                }
            }
        }
    }

    // Sort by commit date descending (most recent first)
    // Parse the timestamp from author/committer line
    commits_to_show.sort_by(|a, b| {
        let ts_a = parse_timestamp_from_ident(&a.1.author);
        let ts_b = parse_timestamp_from_ident(&b.1.author);
        ts_b.cmp(&ts_a)
    });

    for (i, (oid, commit)) in commits_to_show.iter().enumerate() {
        msg.push('\n');
        msg.push_str(&format!("commit {}\n", oid.to_hex()));
        msg.push_str(&format!(
            "Author: {}\n",
            format_author_for_log(&commit.author)
        ));
        msg.push_str(&format!(
            "Date:   {}\n",
            format_date_for_log(&commit.author)
        ));
        msg.push('\n');
        for line in commit.message.trim_end().lines() {
            msg.push_str(&format!("    {}\n", line));
        }
        // Add trailing blank line only after the last commit
        if i == commits_to_show.len() - 1 {
            msg.push('\n');
        }
    }

    Ok(msg)
}

/// Extract timestamp (epoch seconds) from a git ident line like "Name <email> 1234567890 +0000"
fn parse_timestamp_from_ident(ident: &str) -> i64 {
    // Format: "Name <email> timestamp timezone"
    if let Some(after_email) = ident.rfind('>') {
        let rest = ident[after_email + 1..].trim();
        if let Some(space) = rest.find(' ') {
            rest[..space].parse().unwrap_or(0)
        } else {
            rest.parse().unwrap_or(0)
        }
    } else {
        0
    }
}

/// Format the author name/email portion from an ident line for display.
fn format_author_for_log(ident: &str) -> String {
    // "Name <email> timestamp tz" → "Name <email>"
    if let Some(pos) = ident.rfind('>') {
        ident[..=pos].to_string()
    } else {
        ident.to_string()
    }
}

/// Format the date portion from an ident line for display.
fn format_date_for_log(ident: &str) -> String {
    if let Some(after_email) = ident.rfind('>') {
        let rest = ident[after_email + 1..].trim();
        // rest is "timestamp timezone"
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() == 2 {
            if let Ok(epoch) = parts[0].parse::<i64>() {
                // Parse timezone offset
                let tz_str = parts[1];
                let tz_secs = parse_tz_offset(tz_str);
                // Format as "Thu Apr  7 15:14:13 2005 -0700"
                if let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(epoch) {
                    let offset = time::UtcOffset::from_whole_seconds(tz_secs)
                        .unwrap_or(time::UtcOffset::UTC);
                    let dt = dt.to_offset(offset);
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
                    let day = dt.day();
                    let (h, m, s) = (dt.hour(), dt.minute(), dt.second());
                    let year = dt.year();
                    return format!(
                        "{weekday} {month} {day:>2} {h:02}:{m:02}:{s:02} {year} {tz_str}"
                    );
                }
            }
        }
    }
    String::new()
}

fn parse_tz_offset(tz: &str) -> i32 {
    // "+0700" or "-0530"
    if tz.len() < 5 {
        return 0;
    }
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let hours: i32 = tz[1..3].parse().unwrap_or(0);
    let mins: i32 = tz[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + mins * 60)
}

/// Squash merge: stage changes but don't commit.
fn do_squash(
    repo: &Repository,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    // For a simple fast-forward squash, stage the merge target's tree
    let commit_obj = repo.odb.read(&merge_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let entries = tree_to_index_entries(repo, &commit.tree, "")?;
    let mut new_index = Index::new();
    new_index.entries = entries;
    new_index.sort();

    if let Some(ref wt) = repo.work_tree {
        checkout_entries(repo, wt, &new_index, None)?;
    }
    refresh_index_stat_cache_from_worktree(repo, &mut new_index)?;
    repo.write_index(&mut new_index)?;

    // Write SQUASH_MSG
    let msg = build_squash_msg(repo, head_oid, &[merge_oid])?;
    fs::write(repo.git_dir.join("SQUASH_MSG"), &msg)?;

    if !args.quiet {
        eprintln!(
            "Squash commit -- not updating HEAD\n\
             Updating {}..{}",
            &head_oid.to_hex()[..7],
            &merge_oid.to_hex()[..7]
        );
    }
    run_post_merge_hook(repo, true);
    Ok(())
}

/// Squash from a three-way merge result.
fn do_squash_from_merge(
    repo: &Repository,
    mut index: Index,
    _head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    repo.write_index(&mut index)?;

    let msg = build_squash_msg(repo, head_oid, &[merge_oid])?;
    fs::write(repo.git_dir.join("SQUASH_MSG"), &msg)?;

    if !args.quiet {
        eprintln!("Squash commit -- not updating HEAD");
    }
    run_post_merge_hook(repo, true);
    Ok(())
}

/// Abort an in-progress merge.
fn merge_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("MERGE_HEAD").exists() {
        bail!("There is no merge to abort (MERGE_HEAD missing).");
    }

    // Restore to ORIG_HEAD if available, otherwise HEAD
    let restore_oid = if let Some(orig) = grit_lib::state::read_orig_head(git_dir)? {
        orig
    } else {
        let head = resolve_head(git_dir)?;
        match head.oid() {
            Some(oid) => *oid,
            None => bail!("cannot determine HEAD to restore"),
        }
    };

    // Restore index and working tree from the restore commit
    let commit_obj = repo.odb.read(&restore_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
    let mut index = Index::new();
    index.entries = entries;
    index.sort();

    if let Some(ref wt) = repo.work_tree {
        checkout_entries(&repo, wt, &index, None)?;
    }
    refresh_index_stat_cache_from_worktree(&repo, &mut index)?;
    repo.write_index(&mut index)?;

    // Clean up merge state files
    let _ = fs::remove_file(git_dir.join("MERGE_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(git_dir.join("MERGE_MODE"));

    Ok(())
}

/// Quit the current merge: clean up merge state files but leave HEAD, index,
/// and working tree untouched.
fn merge_quit() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    // Clean up merge state files
    let _ = fs::remove_file(git_dir.join("MERGE_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(git_dir.join("MERGE_MODE"));
    let _ = fs::remove_file(git_dir.join("AUTO_MERGE"));

    Ok(())
}

/// Continue a merge after conflict resolution (delegates to commit).
fn merge_continue(message: Option<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !git_dir.join("MERGE_HEAD").exists() {
        bail!("There is no merge in progress (MERGE_HEAD missing).");
    }

    // Check that index has no unmerged entries
    let index = match repo.load_index() {
        Ok(idx) => idx,
        Err(e) => bail!("cannot load index: {}", e),
    };

    let has_conflicts = index.entries.iter().any(|e| e.stage() != 0);
    if has_conflicts {
        bail!("you need to resolve all merge conflicts before continuing");
    }

    // Build the commit via the existing commit machinery
    // Read MERGE_HEAD, MERGE_MSG
    let merge_heads = grit_lib::state::read_merge_heads(git_dir)?;
    let head = resolve_head(git_dir)?;
    let head_oid = head.oid().copied().context("HEAD has no commit")?;

    let msg = if let Some(m) = message {
        ensure_trailing_newline(&m)
    } else if let Some(merge_msg) = grit_lib::state::read_merge_msg(git_dir)? {
        merge_msg
    } else {
        bail!("no merge message found (use -m to provide one)");
    };

    let tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let author = resolve_ident(&config, "author", now)?;
    let committer = resolve_ident(&config, "committer", now)?;

    let mut parents = vec![head_oid];
    parents.extend(merge_heads);

    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding: None,
        message: msg.clone(),
        raw_message: None,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;
    update_head(git_dir, &head, &commit_oid)?;

    // Clean up
    let _ = fs::remove_file(git_dir.join("MERGE_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(git_dir.join("MERGE_MODE"));

    let branch = head.branch_name().unwrap_or("HEAD");
    let short = &commit_oid.to_hex()[..7];
    let first_line = msg.lines().next().unwrap_or("");
    println!("[{branch} {short}] {first_line}");

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

struct MergeResult {
    index: Index,
    has_conflicts: bool,
    /// Files with conflict markers: (path, content).
    conflict_files: Vec<(String, Vec<u8>)>,
    conflict_descriptions: Vec<ConflictDescription>,
}

/// One recorded merge conflict for stdout and for remerge-diff headers.
#[derive(Debug, Clone)]
pub(crate) struct ConflictDescription {
    /// Short type tag: `content`, `modify/delete`, `rename/rename`, …
    pub kind: &'static str,
    /// Text after `CONFLICT (kind): ` on the standard merge output line.
    pub body: String,
    /// Path or label replay uses in error messages (legacy second tuple field).
    pub subject_path: String,
    /// When set, remerge-diff matches this path to a diff entry (e.g. rename/rename uses the source path).
    pub remerge_anchor_path: Option<String>,
    /// For `rename/rename(1to2)`: our-side rename destination in the index (mechanical merge tree).
    pub rename_rr_ours_dest: Option<String>,
    /// For `rename/rename(1to2)`: their-side rename destination in the index.
    pub rename_rr_theirs_dest: Option<String>,
}

impl ConflictDescription {
    /// Full line body prefixed for `remerge` diff headers (matches Git).
    #[must_use]
    pub fn remerge_header_line(&self) -> String {
        format!("remerge CONFLICT ({}): {}", self.kind, self.body)
    }
}

/// Tree-merge result exported for replay-style callers.
pub(crate) struct ReplayTreeMergeResult {
    /// Merged index entries, including conflict stages when unresolved.
    pub index: Index,
    /// Whether the merge produced conflicts.
    pub has_conflicts: bool,
    /// Files with conflict marker content to materialize in worktree.
    pub conflict_files: Vec<(String, Vec<u8>)>,
    /// Human-readable conflict summaries.
    pub conflict_descriptions: Vec<ConflictDescription>,
}

#[derive(Debug)]
struct InternalMergeExecutionError;

impl std::fmt::Display for InternalMergeExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to execute internal merge")
    }
}

impl std::error::Error for InternalMergeExecutionError {}

#[derive(Clone, Copy)]
struct ConflictLabels<'a> {
    ours: &'a str,
    base: &'a str,
}

fn resolve_conflict_labels(
    repo: &Repository,
    theirs_name: &str,
    base_label_prefix: &str,
) -> ConflictLabels<'static> {
    let ours = if theirs_name == "Temporary merge branch 2" {
        "Temporary merge branch 1"
    } else {
        "HEAD"
    };

    let base = if base_label_prefix == "empty tree" {
        "empty tree".to_string()
    } else if matches!(resolve_conflict_style(repo), ConflictStyle::ZealousDiff3)
        && base_label_prefix.chars().all(|c| c.is_ascii_hexdigit())
    {
        base_label_prefix.to_string()
    } else {
        format!("{base_label_prefix}:content")
    };

    let ours_static: &'static str = Box::leak(ours.to_string().into_boxed_str());
    let base_static: &'static str = Box::leak(base.into_boxed_str());

    ConflictLabels {
        ours: ours_static,
        base: base_static,
    }
}

pub(crate) fn is_internal_merge_execution_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<InternalMergeExecutionError>()
            .is_some()
    })
}

#[derive(Debug, Clone)]
enum PathMergeBehavior {
    Default,
    BinaryNoMerge,
    Union,
    CustomDriver { command: String },
    CustomDriverMissing { name: String },
}

fn resolve_path_merge_behavior(repo: &Repository, path: &str) -> PathMergeBehavior {
    let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
        return PathMergeBehavior::Default;
    };

    let attrs = repo
        .work_tree
        .as_deref()
        .map(grit_lib::crlf::load_gitattributes)
        .unwrap_or_default();
    let file_attrs = grit_lib::crlf::get_file_attrs(&attrs, path, &config);

    match &file_attrs.merge {
        MergeAttr::Unset => PathMergeBehavior::BinaryNoMerge,
        MergeAttr::Driver(name) => {
            if name == "union" {
                PathMergeBehavior::Union
            } else {
                let key = format!("merge.{name}.driver");
                if let Some(command) = config.get(&key) {
                    PathMergeBehavior::CustomDriver { command }
                } else {
                    PathMergeBehavior::CustomDriverMissing { name: name.clone() }
                }
            }
        }
        MergeAttr::Unspecified => {
            if let Some(command) = config.get("merge.default.driver") {
                PathMergeBehavior::CustomDriver { command }
            } else {
                PathMergeBehavior::Default
            }
        }
    }
}

fn resolve_marker_size_for_path(
    repo: &Repository,
    path: &str,
    ours_label: &str,
    theirs_label: &str,
    marker_warnings: &mut Vec<String>,
) -> usize {
    let mut warning = String::new();
    let size = if let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) {
        let attrs = repo
            .work_tree
            .as_deref()
            .map(grit_lib::crlf::load_gitattributes)
            .unwrap_or_default();
        let file_attrs = grit_lib::crlf::get_file_attrs(&attrs, path, &config);
        parse_conflict_marker_size(
            Some(&file_attrs),
            ours_label,
            theirs_label,
            Some(&mut warning),
        )
    } else {
        parse_conflict_marker_size(None, ours_label, theirs_label, None)
    };
    if !warning.is_empty() {
        marker_warnings.push(warning);
    }
    size
}

#[allow(clippy::too_many_arguments)]
fn execute_custom_merge_driver(
    command_template: &str,
    path: &str,
    base_content: &[u8],
    ours_content: &[u8],
    theirs_content: &[u8],
    base_name: &str,
    ours_name: &str,
    theirs_name: &str,
) -> Result<(Vec<u8>, i32)> {
    let mut base_tmp = NamedTempFile::new().context("creating merge driver base tempfile")?;
    let mut ours_tmp = NamedTempFile::new().context("creating merge driver ours tempfile")?;
    let mut theirs_tmp = NamedTempFile::new().context("creating merge driver theirs tempfile")?;

    base_tmp
        .write_all(base_content)
        .context("writing base tempfile content")?;
    ours_tmp
        .write_all(ours_content)
        .context("writing ours tempfile content")?;
    theirs_tmp
        .write_all(theirs_content)
        .context("writing theirs tempfile content")?;
    base_tmp.flush()?;
    ours_tmp.flush()?;
    theirs_tmp.flush()?;

    let base_path = base_tmp.path().to_string_lossy().into_owned();
    let ours_path = ours_tmp.path().to_string_lossy().into_owned();
    let theirs_path = theirs_tmp.path().to_string_lossy().into_owned();

    let command = command_template
        .replace("%O", &shell_escape_single_quoted(&base_path))
        .replace("%A", &shell_escape_single_quoted(&ours_path))
        .replace("%B", &shell_escape_single_quoted(&theirs_path))
        .replace("%P", &shell_escape_single_quoted(path))
        .replace("%S", &shell_escape_single_quoted(base_name))
        .replace("%X", &shell_escape_single_quoted(ours_name))
        .replace("%Y", &shell_escape_single_quoted(theirs_name));

    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .status()
        .context("executing merge driver command")?;
    let exit_code = match status.code() {
        Some(code) if code >= 128 => {
            return Err(InternalMergeExecutionError.into());
        }
        Some(code) => code,
        None => {
            return Err(InternalMergeExecutionError.into());
        }
    };
    let merged = fs::read(ours_tmp.path()).context("reading merge driver output")?;
    Ok((merged, exit_code))
}

fn shell_escape_single_quoted(value: &str) -> String {
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

fn parse_conflict_marker_size(
    file_attrs: Option<&grit_lib::crlf::FileAttrs>,
    ours_label: &str,
    theirs_label: &str,
    warning_out: Option<&mut String>,
) -> usize {
    if let Some(attrs) = file_attrs {
        if let Some(raw) = &attrs.conflict_marker_size {
            if let Ok(parsed) = raw.parse::<usize>() {
                return parsed;
            }
            if let Some(out) = warning_out {
                *out = format!("warning: invalid marker-size '{raw}', expecting an integer");
            }
            return 7;
        }
    }

    if ours_label.starts_with("Temporary merge branch")
        || theirs_label.starts_with("Temporary merge branch")
    {
        9
    } else {
        7
    }
}

/// Rename detection settings for tree merge (CLI and `merge.renames` / `diff.renames`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MergeRenameOptions {
    /// When false, skip all rename detection (exact and similarity-based).
    pub detect: bool,
    /// Minimum similarity percentage (0–100) for similarity-based renames.
    pub threshold: u32,
}

impl MergeRenameOptions {
    /// Load defaults from config: `merge.renames` overrides `diff.renames`; threshold is 50%.
    pub fn from_config(repo: &Repository) -> Self {
        let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
        let detect = merge_renames_enabled_from_config(config.as_ref());
        Self {
            detect,
            threshold: 50,
        }
    }
}

fn merge_renames_enabled_from_config(config: Option<&ConfigSet>) -> bool {
    let Some(c) = config else {
        return true;
    };
    if let Some(v) = c.get("merge.renames") {
        return config_value_enables_renames(&v);
    }
    if let Some(v) = c.get("diff.renames") {
        return config_value_enables_renames(&v);
    }
    true
}

fn config_value_enables_renames(val: &str) -> bool {
    let lowered = val.trim().to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "true" | "yes" | "on" | "1" | "" | "copies" | "copy"
    )
}

/// Build rename maps from base to each side.
///
/// Detects renames by looking for base blobs that appear at different paths
/// in a side (exact OID match), plus similarity-based rename detection for
/// cases where the renamed file was also modified.
///
/// Returns (ours_renames, theirs_renames) where each map goes from
/// old_path (in base) → new_path (in that side).
fn detect_merge_renames(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    rename_opts: MergeRenameOptions,
) -> (HashMap<Vec<u8>, Vec<u8>>, HashMap<Vec<u8>, Vec<u8>>) {
    if !rename_opts.detect {
        return (HashMap::new(), HashMap::new());
    }
    let threshold = rename_opts.threshold.min(100);
    // Read merge.renamelimit or fall back to diff.renamelimit
    let rename_limit: usize = {
        let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
        config
            .as_ref()
            .and_then(|c| c.get("merge.renamelimit"))
            .or_else(|| config.as_ref().and_then(|c| c.get("diff.renamelimit")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000)
    };
    let zero_oid = ObjectId::from_bytes(&[0u8; 20]).unwrap();

    // Build diff entries from base to side, handling the "add-source" pattern:
    // If base has path P with OID X, and side has path P with a DIFFERENT OID Y,
    // but side also has path Q with OID X (exact match), then:
    //   - P was renamed to Q (Deleted P + Added Q)
    //   - A new file was added at P (the Modified becomes an Add)
    let build_diff = |side: &HashMap<Vec<u8>, IndexEntry>| -> Vec<DiffEntry> {
        // First, build an OID → paths map for the side to detect where base blobs moved
        let mut side_oid_to_paths: HashMap<ObjectId, Vec<Vec<u8>>> = HashMap::new();
        for (path, entry) in side {
            side_oid_to_paths
                .entry(entry.oid)
                .or_default()
                .push(path.clone());
        }

        // Find base entries whose OID appears at a different path in the side
        let mut exact_renames: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
        for (base_path, base_entry) in base {
            if let Some(side_entry) = side.get(base_path) {
                // If the same blob is still present at the original path, this
                // source was not renamed away; don't treat additional copies as
                // exact renames from this path.
                if side_entry.oid == base_entry.oid && side_entry.mode == base_entry.mode {
                    continue;
                }
            }
            if let Some(side_paths) = side_oid_to_paths.get(&base_entry.oid) {
                for sp in side_paths {
                    if sp != base_path && !base.contains_key(sp) {
                        // base_path's content appeared at a new path sp in side
                        exact_renames.insert(base_path.clone(), sp.clone());
                        break;
                    }
                }
            }
        }

        let mut entries = Vec::new();
        let mut all_paths = BTreeSet::new();
        all_paths.extend(base.keys());
        all_paths.extend(side.keys());

        // Track which paths are rename targets (don't emit them as plain Added)
        let rename_targets: BTreeSet<Vec<u8>> = exact_renames.values().cloned().collect();
        // Track which paths are rename sources (emit as Deleted)
        let rename_sources: BTreeSet<Vec<u8>> = exact_renames.keys().cloned().collect();

        for path in all_paths {
            let b = base.get(path);
            let s = side.get(path);
            let path_str = String::from_utf8_lossy(path).to_string();
            match (b, s) {
                (Some(be), None) => {
                    // Deleted in side
                    if !rename_sources.contains(path) {
                        entries.push(DiffEntry {
                            status: DiffStatus::Deleted,
                            old_path: Some(path_str),
                            new_path: None,
                            old_mode: format!("{:06o}", be.mode),
                            new_mode: String::new(),
                            old_oid: be.oid,
                            new_oid: zero_oid,
                            score: None,
                        });
                    }
                    // If it's a rename source, we handle it via the exact_renames map
                }
                (None, Some(se)) => {
                    // Added in side
                    if !rename_targets.contains(path) {
                        entries.push(DiffEntry {
                            status: DiffStatus::Added,
                            old_path: None,
                            new_path: Some(path_str),
                            old_mode: String::new(),
                            new_mode: format!("{:06o}", se.mode),
                            old_oid: zero_oid,
                            new_oid: se.oid,
                            score: None,
                        });
                    }
                }
                (Some(be), Some(se)) => {
                    // If this is a rename source (content moved elsewhere) and
                    // the content at this path changed, treat the old content as
                    // "deleted" (it moved) and the new content as "added" (new file).
                    if rename_sources.contains(path) && be.oid != se.oid {
                        // The old content moved away → emit Deleted for rename detection
                        entries.push(DiffEntry {
                            status: DiffStatus::Deleted,
                            old_path: Some(path_str.clone()),
                            new_path: None,
                            old_mode: format!("{:06o}", be.mode),
                            new_mode: String::new(),
                            old_oid: be.oid,
                            new_oid: zero_oid,
                            score: None,
                        });
                    }
                }
                _ => {}
            }
        }
        entries
    };

    let extract_renames = |side: &HashMap<Vec<u8>, IndexEntry>| -> HashMap<Vec<u8>, Vec<u8>> {
        // First, exact OID-based renames
        let mut side_oid_to_paths: HashMap<ObjectId, Vec<Vec<u8>>> = HashMap::new();
        for (path, entry) in side {
            side_oid_to_paths
                .entry(entry.oid)
                .or_default()
                .push(path.clone());
        }

        let mut map: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
        let mut matched_targets: BTreeSet<Vec<u8>> = BTreeSet::new();

        for (base_path, base_entry) in base {
            if side.contains_key(base_path) {
                // Path still exists in side — check if it's an add-source pattern
                let side_entry = &side[base_path];
                if side_entry.oid == base_entry.oid {
                    continue; // Same content, not renamed
                }
                // Content at base_path changed. Check if original content moved.
                if let Some(side_paths) = side_oid_to_paths.get(&base_entry.oid) {
                    for sp in side_paths {
                        if sp != base_path
                            && !base.contains_key(sp)
                            && !matched_targets.contains(sp)
                        {
                            map.insert(base_path.clone(), sp.clone());
                            matched_targets.insert(sp.clone());
                            break;
                        }
                    }
                }
            } else {
                // Path doesn't exist in side — look for exact OID match at new path
                if let Some(side_paths) = side_oid_to_paths.get(&base_entry.oid) {
                    for sp in side_paths {
                        if !base.contains_key(sp) && !matched_targets.contains(sp) {
                            map.insert(base_path.clone(), sp.clone());
                            matched_targets.insert(sp.clone());
                            break;
                        }
                    }
                }
            }
        }

        // Now do similarity-based rename detection for remaining unmatched deletions
        let diff_entries = build_diff(side);
        // Check rename limit: count deleted and added entries
        let n_deleted = diff_entries
            .iter()
            .filter(|e| matches!(e.status, DiffStatus::Deleted))
            .count();
        let n_added = diff_entries
            .iter()
            .filter(|e| matches!(e.status, DiffStatus::Added))
            .count();
        let detected = if n_deleted > rename_limit || n_added > rename_limit {
            // Rename detection matrix too large, skip similarity detection
            Vec::new()
        } else {
            detect_renames(&repo.odb, diff_entries, threshold)
        };
        for e in detected {
            if matches!(e.status, DiffStatus::Renamed) {
                if let (Some(old), Some(new)) = (&e.old_path, &e.new_path) {
                    let old_bytes = old.as_bytes().to_vec();
                    let new_bytes = new.as_bytes().to_vec();
                    if !map.contains_key(&old_bytes) && !matched_targets.contains(&new_bytes) {
                        map.insert(old_bytes, new_bytes.clone());
                        matched_targets.insert(new_bytes);
                    }
                }
            }
        }

        map
    };

    let ours_renames = extract_renames(ours);
    let theirs_renames = extract_renames(theirs);

    (ours_renames, theirs_renames)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MergeDirectoryRenamesMode {
    /// Use repository config (merge.directoryRenames).
    FromConfig,
    /// Force directory rename handling on.
    #[allow(dead_code)]
    Enabled,
    /// Force directory rename handling off.
    Disabled,
}

fn merge_directory_renames_enabled(repo: &Repository) -> bool {
    let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
        return false;
    };
    let Some(raw) = config
        .get("merge.directoryrenames")
        .or_else(|| config.get("merge.directoryRenames"))
    else {
        return false;
    };
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1" | "conflict"
    )
}

fn merge_directory_renames_enabled_for_mode(
    repo: &Repository,
    mode: MergeDirectoryRenamesMode,
) -> bool {
    match mode {
        MergeDirectoryRenamesMode::Enabled => true,
        MergeDirectoryRenamesMode::Disabled => false,
        MergeDirectoryRenamesMode::FromConfig => merge_directory_renames_enabled(repo),
    }
}

fn parent_dir(path: &[u8]) -> Option<Vec<u8>> {
    let slash = path.iter().rposition(|b| *b == b'/')?;
    if slash == 0 {
        return None;
    }
    Some(path[..slash].to_vec())
}

fn build_directory_rename_map(renames: &HashMap<Vec<u8>, Vec<u8>>) -> HashMap<Vec<u8>, Vec<u8>> {
    let mut dir_map: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
    let mut conflicting_sources: BTreeSet<Vec<u8>> = BTreeSet::new();

    for (old_path, new_path) in renames {
        let (Some(old_dir), Some(new_dir)) = (parent_dir(old_path), parent_dir(new_path)) else {
            continue;
        };
        if old_dir == new_dir || conflicting_sources.contains(&old_dir) {
            continue;
        }
        match dir_map.get(&old_dir) {
            None => {
                dir_map.insert(old_dir, new_dir);
            }
            Some(existing) if *existing == new_dir => {}
            Some(_) => {
                dir_map.remove(&old_dir);
                conflicting_sources.insert(old_dir);
            }
        }
    }

    dir_map
}

fn remap_path_by_directory_renames(
    path: &[u8],
    dir_renames: &HashMap<Vec<u8>, Vec<u8>>,
) -> Option<Vec<u8>> {
    let mut best_match: Option<(&Vec<u8>, &Vec<u8>)> = None;
    for (old_dir, new_dir) in dir_renames {
        if path.len() <= old_dir.len() || !path.starts_with(old_dir) {
            continue;
        }
        if path.get(old_dir.len()) != Some(&b'/') {
            continue;
        }
        let should_replace = match best_match {
            None => true,
            Some((best_old, _)) => old_dir.len() > best_old.len(),
        };
        if should_replace {
            best_match = Some((old_dir, new_dir));
        }
    }

    let (old_dir, new_dir) = best_match?;
    let suffix = &path[old_dir.len() + 1..];
    let mut rewritten = new_dir.clone();
    rewritten.push(b'/');
    rewritten.extend_from_slice(suffix);
    Some(rewritten)
}

fn apply_directory_renames_to_side(
    base: &HashMap<Vec<u8>, IndexEntry>,
    side_entries: &mut HashMap<Vec<u8>, IndexEntry>,
    side_renames: &mut HashMap<Vec<u8>, Vec<u8>>,
    opposite_dir_renames: &HashMap<Vec<u8>, Vec<u8>>,
) {
    if opposite_dir_renames.is_empty() {
        return;
    }

    for target_path in side_renames.values_mut() {
        if let Some(remapped) = remap_path_by_directory_renames(target_path, opposite_dir_renames) {
            *target_path = remapped;
        }
    }

    let original_paths: Vec<Vec<u8>> = side_entries.keys().cloned().collect();
    for old_path in original_paths {
        if base.contains_key(&old_path) {
            continue;
        }
        let Some(new_path) = remap_path_by_directory_renames(&old_path, opposite_dir_renames)
        else {
            continue;
        };
        if new_path == old_path || side_entries.contains_key(&new_path) {
            continue;
        }
        let Some(mut entry) = side_entries.remove(&old_path) else {
            continue;
        };
        entry.path = new_path.clone();
        side_entries.insert(new_path, entry);
    }
}

/// Perform tree-level three-way merge.
///
/// For directory/file conflicts, unmerged entries are placed at `path~SUFFIX` where `SUFFIX`
/// is the full hex OID of the commit whose tree still has a **file** at that path (not the
/// side that turned the path into a directory).
fn merge_trees(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    _head: &HeadState,
    their_name: &str,
    base_label_prefix: &str,
    merge_ours_oid_hex: &str,
    merge_theirs_oid_hex: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    merge_renormalize: bool,
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_space_at_eol: bool,
    ignore_cr_at_eol: bool,
    merge_directory_renames_mode: MergeDirectoryRenamesMode,
    rename_options: MergeRenameOptions,
    forced_branch_labels: Option<(String, String)>,
) -> Result<MergeResult> {
    // Detect renames on each side
    let (mut ours_renames, mut theirs_renames) =
        detect_merge_renames(repo, base, ours, theirs, rename_options);
    let mut ours_entries = ours.clone();
    let mut theirs_entries = theirs.clone();

    if merge_directory_renames_enabled_for_mode(repo, merge_directory_renames_mode) {
        let ours_dir_renames = build_directory_rename_map(&ours_renames);
        let theirs_dir_renames = build_directory_rename_map(&theirs_renames);
        apply_directory_renames_to_side(
            base,
            &mut ours_entries,
            &mut ours_renames,
            &theirs_dir_renames,
        );
        apply_directory_renames_to_side(
            base,
            &mut theirs_entries,
            &mut theirs_renames,
            &ours_dir_renames,
        );
    }

    // When both sides independently renamed the same source path to the same
    // destination, this is a rename/rename(1to1) and not a rename/add.
    // Drop stale "add-source" entries that still exist at the source path in
    // the transformed snapshots so later passes don't incorrectly treat them
    // as independent additions.
    for (base_path, ours_new_path) in &ours_renames {
        if theirs_renames.get(base_path) == Some(ours_new_path) {
            // Only strip paths that are still the *base* version at the rename source.
            // If one side replaced the source path (e.g. file `a` → symlink `a` after
            // renaming content to `e`), removing it here would drop that entry and
            // break rename/rename(1to1) + symlink-at-source merges (t6430).
            if let Some(be) = base.get(base_path) {
                if let Some(ours_e) = ours_entries.get(base_path) {
                    if ours_e.oid == be.oid && ours_e.mode == be.mode {
                        ours_entries.remove(base_path);
                    }
                }
                if let Some(theirs_e) = theirs_entries.get(base_path) {
                    if theirs_e.oid == be.oid && theirs_e.mode == be.mode {
                        theirs_entries.remove(base_path);
                    }
                }
            }
        }
    }

    // Track which paths are handled via rename logic so we don't double-process
    let mut handled_paths: BTreeSet<Vec<u8>> = BTreeSet::new();

    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours_entries.keys().cloned());
    all_paths.extend(theirs_entries.keys().cloned());

    let mut index = Index::new();
    let mut has_conflicts = false;
    let mut conflict_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut conflict_descriptions: Vec<ConflictDescription> = Vec::new();

    let labels = resolve_conflict_labels(repo, their_name, base_label_prefix);
    let base_label = labels.base;
    let ours_label: &str = match &forced_branch_labels {
        Some((o, _)) => o.as_str(),
        None => labels.ours,
    };
    let their_name: &str = match &forced_branch_labels {
        Some((_, t)) => t.as_str(),
        None => their_name,
    };
    let has_descendant = |tree: &HashMap<Vec<u8>, IndexEntry>, path: &[u8]| -> bool {
        tree.keys().any(|candidate| {
            candidate.len() > path.len()
                && candidate.starts_with(path)
                && candidate.get(path.len()) == Some(&b'/')
        })
    };

    // First pass: handle rename cases
    // Case 1: ours renamed base_path → ours_new_path; theirs may have modified base_path
    for (base_path, ours_new_path) in &ours_renames {
        handled_paths.insert(base_path.clone());
        // The new path on ours side is handled here too (don't treat as add/add)
        handled_paths.insert(ours_new_path.clone());

        let be = base.get(base_path);
        let oe = ours_entries.get(ours_new_path); // The renamed file in ours
        let te = theirs_entries.get(base_path); // Theirs' version at original path
        let mut symlink_at_rename_source = false;

        if let (Some(be), Some(oe)) = (be, oe) {
            let mut resolved_entry_at_new: Option<IndexEntry> = None;
            let mut has_conflict_at_new = false;

            // Rename/rename(1to1) to the same destination, with a new entry left at the
            // original path on theirs — typically a symlink `a` → `e` while both moved
            // the file content to `e` (see t6430 "rename vs. rename/symlink").
            if theirs_renames.get(base_path) == Some(ours_new_path) {
                if let Some(te_src) = theirs_entries.get(base_path) {
                    if te_src.mode == MODE_SYMLINK {
                        index.entries.push(oe.clone());
                        index.entries.push(te_src.clone());
                        resolved_entry_at_new = Some(oe.clone());
                        symlink_at_rename_source = true;
                    }
                }
            }

            if resolved_entry_at_new.is_some() {
                // Symlink-at-source case handled above; skip three-way content merge on `te`.
            } else if let Some(te) = te {
                // Theirs also has the file at the old path — merge content at new path
                if be.oid == te.oid && be.mode == te.mode {
                    // Theirs didn't modify — just use ours (renamed version)
                    index.entries.push(oe.clone());
                    resolved_entry_at_new = Some(oe.clone());
                } else if oe.oid == te.oid {
                    // Both made same change
                    index.entries.push(oe.clone());
                    resolved_entry_at_new = Some(oe.clone());
                } else {
                    // Both modified — try content merge at new path
                    let path_str = String::from_utf8_lossy(ours_new_path).to_string();
                    match try_content_merge(
                        repo,
                        &path_str,
                        be,
                        oe,
                        te,
                        ours_label,
                        base_label,
                        their_name,
                        favor,
                        diff_algorithm,
                        merge_renormalize,
                        ignore_all_space,
                        ignore_space_change,
                        ignore_space_at_eol,
                        ignore_cr_at_eol,
                    )? {
                        ContentMergeResult::Clean(merged_oid, mode) => {
                            let mut entry = oe.clone();
                            entry.oid = merged_oid;
                            entry.mode = mode;
                            index.entries.push(entry);
                            let mut resolved = oe.clone();
                            resolved.oid = merged_oid;
                            resolved.mode = mode;
                            resolved_entry_at_new = Some(resolved);
                        }
                        ContentMergeResult::Conflict(content) => {
                            has_conflicts = true;
                            has_conflict_at_new = true;
                            let mut be_at_new = be.clone();
                            be_at_new.path = ours_new_path.clone();
                            stage_entry(&mut index, &be_at_new, 1);
                            stage_entry(&mut index, oe, 2);
                            let mut te_at_new = te.clone();
                            te_at_new.path = ours_new_path.clone();
                            stage_entry(&mut index, &te_at_new, 3);
                            conflict_descriptions.push(ConflictDescription {
                                kind: "content",
                                body: format!("Merge conflict in {path_str}"),
                                subject_path: path_str.clone(),
                                remerge_anchor_path: None,
                                rename_rr_ours_dest: None,
                                rename_rr_theirs_dest: None,
                            });
                            conflict_files.push((path_str, content));
                        }
                        ContentMergeResult::BinaryConflict(content) => {
                            has_conflicts = true;
                            has_conflict_at_new = true;
                            let mut be_at_new = be.clone();
                            be_at_new.path = ours_new_path.clone();
                            stage_entry(&mut index, &be_at_new, 1);
                            stage_entry(&mut index, oe, 2);
                            let mut te_at_new = te.clone();
                            te_at_new.path = ours_new_path.clone();
                            stage_entry(&mut index, &te_at_new, 3);
                            let b = format!("{path_str} ({ours_label} vs. {their_name})");
                            conflict_descriptions.push(ConflictDescription {
                                kind: "binary",
                                body: b.clone(),
                                subject_path: b,
                                remerge_anchor_path: None,
                                rename_rr_ours_dest: None,
                                rename_rr_theirs_dest: None,
                            });
                            conflict_files.push((path_str, content));
                        }
                    }
                }
            } else {
                // Theirs deleted the original path. If theirs also renamed the
                // same source to the same destination, treat it as
                // rename/rename(1to1) and merge contents at the destination.
                if theirs_renames.get(base_path) == Some(ours_new_path) {
                    if let Some(te_at_new) = theirs_entries.get(ours_new_path) {
                        if oe.oid == te_at_new.oid && oe.mode == te_at_new.mode {
                            index.entries.push(oe.clone());
                            resolved_entry_at_new = Some(oe.clone());
                        } else {
                            let path_str = String::from_utf8_lossy(ours_new_path).to_string();
                            match try_content_merge(
                                repo,
                                &path_str,
                                be,
                                oe,
                                te_at_new,
                                ours_label,
                                base_label,
                                their_name,
                                favor,
                                diff_algorithm,
                                merge_renormalize,
                                ignore_all_space,
                                ignore_space_change,
                                ignore_space_at_eol,
                                ignore_cr_at_eol,
                            )? {
                                ContentMergeResult::Clean(merged_oid, mode) => {
                                    let mut entry = oe.clone();
                                    entry.oid = merged_oid;
                                    entry.mode = mode;
                                    index.entries.push(entry);
                                    let mut resolved = oe.clone();
                                    resolved.oid = merged_oid;
                                    resolved.mode = mode;
                                    resolved_entry_at_new = Some(resolved);
                                }
                                ContentMergeResult::Conflict(content) => {
                                    has_conflicts = true;
                                    has_conflict_at_new = true;
                                    let mut be_at_new = be.clone();
                                    be_at_new.path = ours_new_path.clone();
                                    stage_entry(&mut index, &be_at_new, 1);
                                    stage_entry(&mut index, oe, 2);
                                    stage_entry(&mut index, te_at_new, 3);
                                    conflict_descriptions.push(ConflictDescription {
                                        kind: "content",
                                        body: format!("Merge conflict in {path_str}"),
                                        subject_path: path_str.clone(),
                                        remerge_anchor_path: None,
                                        rename_rr_ours_dest: None,
                                        rename_rr_theirs_dest: None,
                                    });
                                    conflict_files.push((path_str, content));
                                }
                                ContentMergeResult::BinaryConflict(content) => {
                                    has_conflicts = true;
                                    has_conflict_at_new = true;
                                    let mut be_at_new = be.clone();
                                    be_at_new.path = ours_new_path.clone();
                                    stage_entry(&mut index, &be_at_new, 1);
                                    stage_entry(&mut index, oe, 2);
                                    stage_entry(&mut index, te_at_new, 3);
                                    let b = format!("{path_str} ({ours_label} vs. {their_name})");
                                    conflict_descriptions.push(ConflictDescription {
                                        kind: "binary",
                                        body: b.clone(),
                                        subject_path: b,
                                        remerge_anchor_path: None,
                                        rename_rr_ours_dest: None,
                                        rename_rr_theirs_dest: None,
                                    });
                                    conflict_files.push((path_str, content));
                                }
                            }
                        }
                    } else {
                        index.entries.push(oe.clone());
                        resolved_entry_at_new = Some(oe.clone());
                    }
                } else {
                    // Theirs deleted the original — ours renamed it → rename/delete conflict
                    has_conflicts = true;
                    has_conflict_at_new = true;
                    let base_path_str = String::from_utf8_lossy(base_path).to_string();
                    let new_path_str = String::from_utf8_lossy(ours_new_path).to_string();
                    // Stage the base and ours versions
                    let mut be_at_new = be.clone();
                    be_at_new.path = ours_new_path.clone();
                    stage_entry(&mut index, &be_at_new, 1);
                    stage_entry(&mut index, oe, 2);
                    // Write ours' content to the working tree
                    if let Ok(obj) = repo.odb.read(&oe.oid) {
                        conflict_files.push((new_path_str.clone(), obj.data));
                    }
                    let body = format!(
                        "{base_path_str} deleted in {their_name} and renamed to {new_path_str} in {ours_label}. Version {ours_label} of {new_path_str} left in tree."
                    );
                    conflict_descriptions.push(ConflictDescription {
                        kind: "rename/delete",
                        body: body.clone(),
                        subject_path: new_path_str.clone(),
                        remerge_anchor_path: None,
                        rename_rr_ours_dest: None,
                        rename_rr_theirs_dest: None,
                    });
                }
            }

            // If theirs also has a NEW file at ours_new_path (add/add at rename target)
            if let (Some(te_at_new), Some(resolved_entry)) = (
                theirs_entries.get(ours_new_path),
                resolved_entry_at_new.as_ref(),
            ) {
                if !base.contains_key(ours_new_path) && !has_conflict_at_new {
                    if theirs_renames.get(base_path) == Some(ours_new_path) {
                        continue;
                    }
                    // Theirs added a file at the same path as ours' rename target.
                    // Compare against the already-resolved rename destination content.
                    if resolved_entry.oid != te_at_new.oid || resolved_entry.mode != te_at_new.mode
                    {
                        let path_str = String::from_utf8_lossy(ours_new_path).to_string();
                        has_conflicts = true;
                        remove_stage_zero_entry(&mut index, ours_new_path);
                        stage_entry(&mut index, resolved_entry, 2);
                        stage_entry(&mut index, te_at_new, 3);
                        let conflict_content = match try_content_merge_add_add(
                            repo,
                            &path_str,
                            resolved_entry,
                            te_at_new,
                            ours_label,
                            their_name,
                            MergeFavor::None,
                            diff_algorithm,
                            merge_renormalize,
                            ignore_all_space,
                            ignore_space_change,
                            ignore_space_at_eol,
                            ignore_cr_at_eol,
                        )? {
                            ContentMergeResult::Clean(merged_oid, _) => {
                                repo.odb.read(&merged_oid)?.data
                            }
                            ContentMergeResult::Conflict(content)
                            | ContentMergeResult::BinaryConflict(content) => content,
                        };
                        conflict_files.push((path_str.clone(), conflict_content));
                        conflict_descriptions.push(ConflictDescription {
                            kind: "rename/add",
                            body: format!("Merge conflict in {path_str}"),
                            subject_path: path_str.clone(),
                            remerge_anchor_path: None,
                            rename_rr_ours_dest: None,
                            rename_rr_theirs_dest: None,
                        });
                    }
                }
            }
        }

        // Handle "add-source" only when theirs also renamed this source path away.
        // If theirs did not rename away (i.e. it only modified the original path),
        // we must not keep a tracked entry at base_path here, or we'd clobber an
        // untracked working-tree file at that path in scenarios like t6414.
        if theirs_renames.contains_key(base_path) && !symlink_at_rename_source {
            if let Some(te_at_base) = theirs_entries.get(base_path) {
                if be.is_none_or(|b| te_at_base.oid != b.oid) {
                    // Theirs has a new/different file at the old path (add-source)
                    // while also renaming the original away from this path.
                    index.entries.push(te_at_base.clone());
                }
            }
        }
    }

    // Case 2: theirs renamed base_path → theirs_new_path; ours may have modified base_path
    for (base_path, theirs_new_path) in &theirs_renames {
        if handled_paths.contains(base_path) {
            // Already handled by ours rename of same path (rename/rename case)
            // Still need to handle theirs_new_path
            if !handled_paths.contains(theirs_new_path) {
                handled_paths.insert(theirs_new_path.clone());
                // If both sides renamed the same file to different names: rename/rename conflict
                if let Some(ours_target) = ours_renames.get(base_path) {
                    if ours_target != theirs_new_path {
                        // rename/rename(1to2) conflict
                        if let (Some(oe), Some(te)) = (
                            ours_entries.get(ours_target),
                            theirs_entries.get(theirs_new_path),
                        ) {
                            let path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                            has_conflicts = true;
                            index.remove(base_path);
                            index.remove(ours_target);
                            index.remove(theirs_new_path);
                            if let Some(be) = base.get(base_path) {
                                stage_entry(&mut index, be, 1);
                            }
                            stage_entry(&mut index, oe, 2);
                            stage_entry(&mut index, te, 3);
                            let base_utf = String::from_utf8_lossy(base_path);
                            let ours_tgt_utf = String::from_utf8_lossy(ours_target);
                            let theirs_tgt_utf = String::from_utf8_lossy(theirs_new_path);
                            let body = format!(
                                "{base_utf} renamed to {ours_tgt_utf} in {ours_label} and to {theirs_tgt_utf} in {their_name}."
                            );
                            conflict_descriptions.push(ConflictDescription {
                                kind: "rename/rename",
                                body: body.clone(),
                                subject_path: path_str.clone(),
                                remerge_anchor_path: Some(base_utf.to_string()),
                                rename_rr_ours_dest: Some(ours_tgt_utf.to_string()),
                                rename_rr_theirs_dest: Some(theirs_tgt_utf.to_string()),
                            });
                            if let Ok(obj) = repo.odb.read(&oe.oid) {
                                conflict_files.push((
                                    String::from_utf8_lossy(ours_target).to_string(),
                                    obj.data,
                                ));
                            }
                            if let Ok(obj) = repo.odb.read(&te.oid) {
                                conflict_files.push((
                                    String::from_utf8_lossy(theirs_new_path).to_string(),
                                    obj.data,
                                ));
                            }
                        }
                    }
                }
            }
            continue;
        }
        handled_paths.insert(base_path.clone());
        handled_paths.insert(theirs_new_path.clone());

        let be = base.get(base_path);
        let te = theirs_entries.get(theirs_new_path); // The renamed file in theirs
        let oe = ours_entries.get(base_path); // Ours' version at original path

        if let (Some(be), Some(te)) = (be, te) {
            let mut resolved_entry_at_new: Option<IndexEntry> = None;
            let mut has_conflict_at_new = false;
            if let Some(oe) = oe {
                // Ours also has the file at the old path — merge content at theirs' new path
                if be.oid == oe.oid && be.mode == oe.mode {
                    // Ours didn't modify — just use theirs (renamed version)
                    index.entries.push(te.clone());
                    resolved_entry_at_new = Some(te.clone());
                } else if oe.oid == te.oid {
                    // Both made same change
                    let mut entry = te.clone();
                    entry.path = theirs_new_path.clone();
                    resolved_entry_at_new = Some(entry.clone());
                    index.entries.push(entry);
                } else {
                    // Both modified — try content merge at new path
                    let path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                    match try_content_merge(
                        repo,
                        &path_str,
                        be,
                        oe,
                        te,
                        ours_label,
                        base_label,
                        their_name,
                        favor,
                        diff_algorithm,
                        merge_renormalize,
                        ignore_all_space,
                        ignore_space_change,
                        ignore_space_at_eol,
                        ignore_cr_at_eol,
                    )? {
                        ContentMergeResult::Clean(merged_oid, mode) => {
                            let mut entry = te.clone();
                            entry.oid = merged_oid;
                            entry.mode = mode;
                            index.entries.push(entry);
                            let mut resolved = te.clone();
                            resolved.oid = merged_oid;
                            resolved.mode = mode;
                            resolved_entry_at_new = Some(resolved);
                        }
                        ContentMergeResult::Conflict(content) => {
                            has_conflicts = true;
                            has_conflict_at_new = true;
                            let mut be_at_new = be.clone();
                            be_at_new.path = theirs_new_path.clone();
                            stage_entry(&mut index, &be_at_new, 1);
                            let mut oe_at_new = oe.clone();
                            oe_at_new.path = theirs_new_path.clone();
                            stage_entry(&mut index, &oe_at_new, 2);
                            stage_entry(&mut index, te, 3);
                            conflict_descriptions.push(ConflictDescription {
                                kind: "content",
                                body: format!("Merge conflict in {path_str}"),
                                subject_path: path_str.clone(),
                                remerge_anchor_path: None,
                                rename_rr_ours_dest: None,
                                rename_rr_theirs_dest: None,
                            });
                            conflict_files.push((path_str, content));
                        }
                        ContentMergeResult::BinaryConflict(content) => {
                            has_conflicts = true;
                            has_conflict_at_new = true;
                            let mut be_at_new = be.clone();
                            be_at_new.path = theirs_new_path.clone();
                            stage_entry(&mut index, &be_at_new, 1);
                            let mut oe_at_new = oe.clone();
                            oe_at_new.path = theirs_new_path.clone();
                            stage_entry(&mut index, &oe_at_new, 2);
                            stage_entry(&mut index, te, 3);
                            let b = format!("{path_str} ({ours_label} vs. {their_name})");
                            conflict_descriptions.push(ConflictDescription {
                                kind: "binary",
                                body: b.clone(),
                                subject_path: b,
                                remerge_anchor_path: None,
                                rename_rr_ours_dest: None,
                                rename_rr_theirs_dest: None,
                            });
                            conflict_files.push((path_str, content));
                        }
                    }
                }
            } else {
                // Ours deleted the original — theirs renamed it → rename/delete conflict
                has_conflicts = true;
                has_conflict_at_new = true;
                let base_path_str = String::from_utf8_lossy(base_path).to_string();
                let new_path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                // Stage the base and theirs versions
                let mut be_at_new = be.clone();
                be_at_new.path = theirs_new_path.clone();
                stage_entry(&mut index, &be_at_new, 1);
                stage_entry(&mut index, te, 3);
                // Write theirs' content to the working tree
                if let Ok(obj) = repo.odb.read(&te.oid) {
                    conflict_files.push((new_path_str.clone(), obj.data));
                }
                let body = format!(
                    "{base_path_str} deleted in {ours_label} and renamed to {new_path_str} in {their_name}. Version {their_name} of {new_path_str} left in tree."
                );
                conflict_descriptions.push(ConflictDescription {
                    kind: "rename/delete",
                    body: body.clone(),
                    subject_path: new_path_str.clone(),
                    remerge_anchor_path: None,
                    rename_rr_ours_dest: None,
                    rename_rr_theirs_dest: None,
                });
            }

            // If ours also has a NEW file at theirs_new_path (add/add at rename target)
            if let (Some(oe_at_new), Some(resolved_entry)) = (
                ours_entries.get(theirs_new_path),
                resolved_entry_at_new.as_ref(),
            ) {
                if !base.contains_key(theirs_new_path)
                    && !has_conflict_at_new
                    && (resolved_entry.oid != oe_at_new.oid
                        || resolved_entry.mode != oe_at_new.mode)
                {
                    let path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                    has_conflicts = true;
                    remove_stage_zero_entry(&mut index, theirs_new_path);
                    stage_entry(&mut index, oe_at_new, 2);
                    stage_entry(&mut index, resolved_entry, 3);
                    let conflict_content = match try_content_merge_add_add(
                        repo,
                        &path_str,
                        oe_at_new,
                        resolved_entry,
                        ours_label,
                        their_name,
                        MergeFavor::None,
                        diff_algorithm,
                        merge_renormalize,
                        ignore_all_space,
                        ignore_space_change,
                        ignore_space_at_eol,
                        ignore_cr_at_eol,
                    )? {
                        ContentMergeResult::Clean(merged_oid, _) => {
                            repo.odb.read(&merged_oid)?.data
                        }
                        ContentMergeResult::Conflict(content)
                        | ContentMergeResult::BinaryConflict(content) => content,
                    };
                    conflict_files.push((path_str.clone(), conflict_content));
                    conflict_descriptions.push(ConflictDescription {
                        kind: "rename/add",
                        body: format!("Merge conflict in {path_str}"),
                        subject_path: path_str.clone(),
                        remerge_anchor_path: None,
                        rename_rr_ours_dest: None,
                        rename_rr_theirs_dest: None,
                    });
                }
            }

            // Handle "add-source": theirs renamed base_path away, but theirs may also
            // have a NEW file at base_path (add-source pattern: rename + add at source).
            // Also handle ours' file at base_path: ours' modification of the original
            // was used for the merge at the rename target, so we should not also keep
            // it at base_path. But theirs' add-source at base_path should be included.
            if let Some(te_at_base) = theirs_entries.get(base_path) {
                if te_at_base.oid != be.oid {
                    // Theirs has a genuinely new file at the old path (add-source)
                    index.entries.push(te_at_base.clone());
                }
            }
        }
    }

    apply_directory_file_conflicts(
        repo,
        their_name,
        ours_label,
        &ours_entries,
        &theirs_entries,
        &mut index,
        &all_paths,
        &mut handled_paths,
        &mut conflict_descriptions,
        &mut conflict_files,
        &mut has_conflicts,
    )?;

    // Second pass: handle non-rename paths
    for path in &all_paths {
        if handled_paths.contains(path) {
            continue;
        }

        let b = base.get(path);
        let o = ours_entries.get(path);
        let t = theirs_entries.get(path);

        // Skip paths that are the "add-source" of a rename on the other side.
        // e.g., if ours renamed old→new, and theirs added a completely new file at old,
        // that new file at old is theirs' addition and should be included as-is.
        // But if this path was the source of a rename and the other side didn't touch it,
        // we already handled it above.

        match (b, o, t) {
            // Both sides identical
            (_, Some(oe), Some(te)) if oe.oid == te.oid && oe.mode == te.mode => {
                index.entries.push(oe.clone());
            }
            // Only theirs changed (base == ours)
            (Some(be), Some(oe), Some(te)) if be.oid == oe.oid && be.mode == oe.mode => {
                index.entries.push(te.clone());
            }
            // Only ours changed (base == theirs)
            (Some(be), Some(oe), Some(te)) if be.oid == te.oid && be.mode == te.mode => {
                index.entries.push(oe.clone());
            }
            // Added only by ours — unless theirs only has paths under this name (directory).
            (None, Some(oe), None) => {
                if oe.mode == MODE_GITLINK {
                    // Submodule replaces a former directory tree (e.g. d/e → gitlink d); not D/F.
                    index.entries.push(oe.clone());
                } else if has_descendant(&theirs_entries, path) {
                    has_conflicts = true;
                    let path_str = String::from_utf8_lossy(path).to_string();
                    let conflict_path = format!("{path_str}~{merge_ours_oid_hex}");
                    let mut oe_c = oe.clone();
                    oe_c.path = conflict_path.as_bytes().to_vec();
                    stage_entry(&mut index, &oe_c, 2);
                    if let Ok(obj) = repo.odb.read(&oe.oid) {
                        conflict_files.push((conflict_path.clone(), obj.data));
                    }
                    conflict_descriptions.push(ConflictDescription {
                        kind: "directory/file",
                        body: format!(
                            "There is a directory with name {path_str} in {their_name}. Adding {path_str} as {conflict_path}"
                        ),
                        subject_path: conflict_path.clone(),
                        remerge_anchor_path: None,
                        rename_rr_ours_dest: None,
                        rename_rr_theirs_dest: None,
                    });
                } else {
                    index.entries.push(oe.clone());
                }
            }
            // Added only by theirs — unless ours only has paths under this name (directory).
            (None, None, Some(te)) => {
                if te.mode == MODE_GITLINK {
                    index.entries.push(te.clone());
                } else if has_descendant(&ours_entries, path) {
                    has_conflicts = true;
                    let path_str = String::from_utf8_lossy(path).to_string();
                    let conflict_path = format!("{path_str}~{merge_theirs_oid_hex}");
                    let mut te_c = te.clone();
                    te_c.path = conflict_path.as_bytes().to_vec();
                    stage_entry(&mut index, &te_c, 3);
                    if let Ok(obj) = repo.odb.read(&te.oid) {
                        conflict_files.push((conflict_path.clone(), obj.data));
                    }
                    conflict_descriptions.push(ConflictDescription {
                        kind: "directory/file",
                        body: format!(
                            "There is a directory with name {path_str} in {ours_label}. Adding {path_str} as {conflict_path}"
                        ),
                        subject_path: conflict_path.clone(),
                        remerge_anchor_path: None,
                        rename_rr_ours_dest: None,
                        rename_rr_theirs_dest: None,
                    });
                } else {
                    index.entries.push(te.clone());
                }
            }
            // Both added same thing
            (None, Some(oe), Some(te)) if oe.oid == te.oid && oe.mode == te.mode => {
                index.entries.push(oe.clone());
            }
            // Deleted by both
            (Some(_), None, None) => {
                // Check if both sides renamed to the same target
                let ours_target = ours_renames.get(path);
                let theirs_target = theirs_renames.get(path);
                if ours_target.is_none() && theirs_target.is_none() {
                    // Truly deleted by both — skip
                }
                // Otherwise already handled above
            }
            // All three differ — content-level merge
            (Some(be), Some(oe), Some(te)) => {
                let path_str = String::from_utf8_lossy(path).to_string();
                match try_content_merge(
                    repo,
                    &path_str,
                    be,
                    oe,
                    te,
                    ours_label,
                    base_label,
                    their_name,
                    favor,
                    diff_algorithm,
                    merge_renormalize,
                    ignore_all_space,
                    ignore_space_change,
                    ignore_space_at_eol,
                    ignore_cr_at_eol,
                )? {
                    ContentMergeResult::Clean(merged_oid, mode) => {
                        let mut entry = oe.clone();
                        entry.oid = merged_oid;
                        entry.mode = mode;
                        index.entries.push(entry);
                    }
                    ContentMergeResult::Conflict(content) => {
                        has_conflicts = true;
                        // Write conflict stages
                        stage_entry(&mut index, be, 1);
                        stage_entry(&mut index, oe, 2);
                        stage_entry(&mut index, te, 3);
                        conflict_descriptions.push(ConflictDescription {
                            kind: "content",
                            body: format!("Merge conflict in {path_str}"),
                            subject_path: path_str.clone(),
                            remerge_anchor_path: None,
                            rename_rr_ours_dest: None,
                            rename_rr_theirs_dest: None,
                        });
                        conflict_files.push((path_str, content));
                    }
                    ContentMergeResult::BinaryConflict(content) => {
                        has_conflicts = true;
                        stage_entry(&mut index, be, 1);
                        stage_entry(&mut index, oe, 2);
                        stage_entry(&mut index, te, 3);
                        let b = format!("{path_str} ({ours_label} vs. {their_name})");
                        conflict_descriptions.push(ConflictDescription {
                            kind: "binary",
                            body: b.clone(),
                            subject_path: b,
                            remerge_anchor_path: None,
                            rename_rr_ours_dest: None,
                            rename_rr_theirs_dest: None,
                        });
                        conflict_files.push((path_str, content));
                    }
                }
            }
            // Delete/modify — conflict only if the surviving side changed
            (Some(be), None, Some(te)) => {
                // Check if ours renamed this file — if so, it's handled above
                if ours_renames.contains_key(path) {
                    // Already handled in rename pass
                } else if be.oid == te.oid && be.mode == te.mode {
                    // Theirs didn't change it, ours deleted → clean delete
                } else if merge_renormalize && blobs_equivalent_after_renormalize(repo, be, te)? {
                    // With merge.renormalize, treat pure normalization-only edits
                    // as unchanged so delete/modify can resolve to delete.
                } else {
                    match favor {
                        MergeFavor::Ours => {
                            // -X ours: keep our decision (delete)
                        }
                        MergeFavor::Theirs => {
                            // -X theirs: keep their version
                            index.entries.push(te.clone());
                        }
                        _ => {
                            // Theirs modified, ours deleted → conflict
                            let path_str = String::from_utf8_lossy(path).to_string();
                            has_conflicts = true;
                            if has_descendant(&ours_entries, path) {
                                // D/F conflict: the old file path now needs to stay a
                                // directory (for entries like `path/file`), so move the
                                // conflict stages and worktree file to a side-path.
                                // Suffix names the commit that still has this path as a file (theirs).
                                let conflict_path = format!("{path_str}~{merge_theirs_oid_hex}");
                                let mut be_conflict = be.clone();
                                be_conflict.path = conflict_path.as_bytes().to_vec();
                                stage_entry(&mut index, &be_conflict, 1);
                                let mut te_conflict = te.clone();
                                te_conflict.path = conflict_path.as_bytes().to_vec();
                                stage_entry(&mut index, &te_conflict, 3);
                                if let Ok(obj) = repo.odb.read(&te.oid) {
                                    conflict_files.push((conflict_path, obj.data));
                                }
                            } else {
                                stage_entry(&mut index, be, 1);
                                stage_entry(&mut index, te, 3);
                            }
                            let body = format!(
                                "{path_str} deleted in {ours_label} and modified in {their_name}.  Version {their_name} of {path_str} left in tree."
                            );
                            conflict_descriptions.push(ConflictDescription {
                                kind: "modify/delete",
                                body,
                                subject_path: path_str.clone(),
                                remerge_anchor_path: None,
                                rename_rr_ours_dest: None,
                                rename_rr_theirs_dest: None,
                            });
                        }
                    }
                }
            }
            (Some(be), Some(oe), None) => {
                // Check if theirs renamed this file — if so, it's handled above
                if theirs_renames.contains_key(path) {
                    // Already handled in rename pass
                } else if be.oid == oe.oid && be.mode == oe.mode {
                    // Ours didn't change it, theirs deleted → clean delete
                } else if merge_renormalize && blobs_equivalent_after_renormalize(repo, be, oe)? {
                    // With merge.renormalize, treat pure normalization-only edits
                    // as unchanged so modify/delete can resolve to delete.
                } else {
                    match favor {
                        MergeFavor::Ours => {
                            // -X ours: keep our version
                            index.entries.push(oe.clone());
                        }
                        MergeFavor::Theirs => {
                            // -X theirs: keep their decision (delete)
                        }
                        _ => {
                            // Ours modified, theirs deleted → conflict
                            let path_str = String::from_utf8_lossy(path).to_string();
                            has_conflicts = true;
                            if has_descendant(&theirs_entries, path) {
                                // D/F conflict: the old file path now needs to stay a
                                // directory (for entries like `path/file`), so move the
                                // conflict stages and worktree file to a side-path.
                                // Suffix names the commit that still has this path as a file (ours).
                                let conflict_path = format!("{path_str}~{merge_ours_oid_hex}");
                                let mut be_conflict = be.clone();
                                be_conflict.path = conflict_path.as_bytes().to_vec();
                                stage_entry(&mut index, &be_conflict, 1);
                                let mut oe_conflict = oe.clone();
                                oe_conflict.path = conflict_path.as_bytes().to_vec();
                                stage_entry(&mut index, &oe_conflict, 2);
                                if let Ok(obj) = repo.odb.read(&oe.oid) {
                                    conflict_files.push((conflict_path, obj.data));
                                }
                            } else {
                                stage_entry(&mut index, be, 1);
                                stage_entry(&mut index, oe, 2);
                            }
                            let body = format!(
                                "{path_str} deleted in {their_name} and modified in {ours_label}.  Version {ours_label} of {path_str} left in tree."
                            );
                            conflict_descriptions.push(ConflictDescription {
                                kind: "modify/delete",
                                body,
                                subject_path: path_str.clone(),
                                remerge_anchor_path: None,
                                rename_rr_ours_dest: None,
                                rename_rr_theirs_dest: None,
                            });
                        }
                    }
                }
            }
            // Both added different content — try content merge with empty base
            (None, Some(oe), Some(te)) => {
                let path_str = String::from_utf8_lossy(path).to_string();
                match try_content_merge_add_add(
                    repo,
                    &path_str,
                    oe,
                    te,
                    ours_label,
                    their_name,
                    favor,
                    diff_algorithm,
                    merge_renormalize,
                    ignore_all_space,
                    ignore_space_change,
                    ignore_space_at_eol,
                    ignore_cr_at_eol,
                )? {
                    ContentMergeResult::Clean(merged_oid, mode) => {
                        let mut entry = oe.clone();
                        entry.oid = merged_oid;
                        entry.mode = mode;
                        index.entries.push(entry);
                    }
                    ContentMergeResult::Conflict(content) => {
                        has_conflicts = true;
                        remove_stage_zero_entry(&mut index, path);
                        stage_entry(&mut index, oe, 2);
                        stage_entry(&mut index, te, 3);
                        conflict_descriptions.push(ConflictDescription {
                            kind: "add/add",
                            body: format!("Merge conflict in {path_str}"),
                            subject_path: path_str.clone(),
                            remerge_anchor_path: None,
                            rename_rr_ours_dest: None,
                            rename_rr_theirs_dest: None,
                        });
                        conflict_files.push((path_str, content));
                    }
                    ContentMergeResult::BinaryConflict(content) => {
                        has_conflicts = true;
                        remove_stage_zero_entry(&mut index, path);
                        stage_entry(&mut index, oe, 2);
                        stage_entry(&mut index, te, 3);
                        let b = format!("{path_str} ({ours_label} vs. {their_name})");
                        conflict_descriptions.push(ConflictDescription {
                            kind: "binary",
                            body: b.clone(),
                            subject_path: b,
                            remerge_anchor_path: None,
                            rename_rr_ours_dest: None,
                            rename_rr_theirs_dest: None,
                        });
                        conflict_files.push((path_str, content));
                    }
                }
            }
            // Shouldn't happen
            (_, None, None) => {}
        }
    }

    index.sort();

    Ok(MergeResult {
        index,
        has_conflicts,
        conflict_files,
        conflict_descriptions,
    })
}

/// Re-merge two parents the same way `git merge` would, returning the resulting tree OID
/// and conflict descriptions for `--remerge-diff` headers.
///
/// `parent1` is treated as the first parent (ours); `parent2` as the second (theirs).
pub(crate) fn remerge_merge_tree(
    repo: &Repository,
    parent1: ObjectId,
    parent2: ObjectId,
) -> Result<(ObjectId, Vec<ConflictDescription>)> {
    let bases = grit_lib::merge_base::merge_bases_first_vs_rest(repo, parent1, &[parent2])?;
    let base_oid = if bases.is_empty() {
        create_empty_base_commit(repo)?
    } else if bases.len() > 1 {
        create_virtual_merge_base(repo, &bases, MergeFavor::None, false)?
    } else {
        bases[0]
    };
    let base_label_prefix = if bases.is_empty() {
        "empty tree".to_string()
    } else if bases.len() > 1 {
        "merged common ancestors".to_string()
    } else {
        short_oid(bases[0])
    };

    let base_tree = commit_tree(repo, base_oid)?;
    let ours_tree = commit_tree(repo, parent1)?;
    let theirs_tree = commit_tree(repo, parent2)?;

    let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
    let ours_entries = tree_to_map(tree_to_index_entries(repo, &ours_tree, "")?);
    let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

    let p1_l = commit_remerge_marker_label(repo, &parent1);
    let p2_l = commit_remerge_marker_label(repo, &parent2);
    let forced = Some((p1_l.clone(), p2_l.clone()));

    let head = HeadState::Detached { oid: parent1 };
    let mut merge_result = merge_trees(
        repo,
        &base_entries,
        &ours_entries,
        &theirs_entries,
        &head,
        "remerge",
        &base_label_prefix,
        &parent1.to_hex(),
        &parent2.to_hex(),
        MergeFavor::None,
        None,
        false,
        false,
        false,
        false,
        false,
        MergeDirectoryRenamesMode::FromConfig,
        MergeRenameOptions::from_config(repo),
        forced,
    )?;

    let labels = resolve_conflict_labels(repo, "remerge", &base_label_prefix);
    let base_merge_label = labels.base;

    materialize_unmerged_entries_for_remerge_tree(
        repo,
        &mut merge_result.index,
        &merge_result.conflict_descriptions,
        base_merge_label,
        &p1_l,
        &p2_l,
    )?;

    let tree_oid = write_tree_from_index(&repo.odb, &merge_result.index, "")?;
    Ok((tree_oid, merge_result.conflict_descriptions))
}

fn materialize_unmerged_entries_for_remerge_tree(
    repo: &Repository,
    index: &mut Index,
    conflict_descs: &[ConflictDescription],
    base_label: &str,
    ours_label: &str,
    theirs_label: &str,
) -> Result<()> {
    for desc in conflict_descs {
        if desc.kind != "rename/rename" {
            continue;
        }
        let (Some(anchor), Some(ours_dest), Some(theirs_dest)) = (
            desc.remerge_anchor_path.as_deref(),
            desc.rename_rr_ours_dest.as_deref(),
            desc.rename_rr_theirs_dest.as_deref(),
        ) else {
            continue;
        };
        let be = index.get(anchor.as_bytes(), 1).cloned();
        let oe = index.get(ours_dest.as_bytes(), 2).cloned();
        let te = index.get(theirs_dest.as_bytes(), 3).cloned();
        if let (Some(_be), Some(oe), Some(te)) = (be, oe, te) {
            index.remove(anchor.as_bytes());
            index.remove(ours_dest.as_bytes());
            index.remove(theirs_dest.as_bytes());
            let mut ours_e = oe;
            ours_e.flags &= 0x0FFF;
            index.add_or_replace(ours_e);
            let mut theirs_e = te;
            theirs_e.flags &= 0x0FFF;
            index.add_or_replace(theirs_e);
        }
    }

    let paths: Vec<Vec<u8>> = index
        .entries
        .iter()
        .filter(|e| e.stage() != 0)
        .map(|e| e.path.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    for path in paths {
        if index.get(&path, 0).is_some() {
            continue;
        }
        let s1 = index.get(&path, 1);
        let s2 = index.get(&path, 2);
        let s3 = index.get(&path, 3);
        let path_str = String::from_utf8_lossy(&path).to_string();
        let new_entry = match (s1, s2, s3) {
            (Some(be), Some(oe), Some(te)) => {
                match try_content_merge(
                    repo,
                    &path_str,
                    be,
                    oe,
                    te,
                    ours_label,
                    base_label,
                    theirs_label,
                    MergeFavor::None,
                    None,
                    false,
                    false,
                    false,
                    false,
                    false,
                )? {
                    ContentMergeResult::Clean(oid, mode) => {
                        let mut e = oe.clone();
                        e.oid = oid;
                        e.mode = mode;
                        e.flags &= 0x0FFF;
                        e
                    }
                    ContentMergeResult::Conflict(content)
                    | ContentMergeResult::BinaryConflict(content) => {
                        let oid = repo.odb.write(ObjectKind::Blob, &content)?;
                        let mut e = oe.clone();
                        e.oid = oid;
                        e.flags &= 0x0FFF;
                        e
                    }
                }
            }
            (Some(_be), None, Some(te)) => {
                let mut e = te.clone();
                e.flags &= 0x0FFF;
                e
            }
            (Some(_be), Some(oe), None) => {
                // modify/delete: recorded merge tree keeps our side's blob (matches Git remerge-diff).
                let mut e = oe.clone();
                e.flags &= 0x0FFF;
                e
            }
            (None, Some(oe), Some(te)) => {
                match try_content_merge_add_add(
                    repo,
                    &path_str,
                    oe,
                    te,
                    ours_label,
                    theirs_label,
                    MergeFavor::None,
                    None,
                    false,
                    false,
                    false,
                    false,
                    false,
                )? {
                    ContentMergeResult::Clean(oid, mode) => {
                        let mut e = oe.clone();
                        e.oid = oid;
                        e.mode = mode;
                        e.flags &= 0x0FFF;
                        e
                    }
                    ContentMergeResult::Conflict(content)
                    | ContentMergeResult::BinaryConflict(content) => {
                        let oid = repo.odb.write(ObjectKind::Blob, &content)?;
                        let mut e = oe.clone();
                        e.oid = oid;
                        e.flags &= 0x0FFF;
                        e
                    }
                }
            }
            _ => continue,
        };
        index.remove(&path);
        index.add_or_replace(new_entry);
    }
    index.sort();
    Ok(())
}

/// Perform a single three-way tree merge with merge-ort style rename handling.
///
/// This is a thin wrapper over the internal merge engine used by `merge` and
/// is intended for sequencer-style commands (such as `replay`) that need to
/// replay commits without touching refs/index/worktree directly.
pub(crate) fn merge_trees_for_replay(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    their_name: &str,
    base_label_prefix: &str,
    merge_ours_oid_hex: &str,
    merge_theirs_oid_hex: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    merge_renormalize: bool,
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_space_at_eol: bool,
    ignore_cr_at_eol: bool,
    merge_directory_renames_mode: MergeDirectoryRenamesMode,
    rename_options: MergeRenameOptions,
) -> Result<ReplayTreeMergeResult> {
    let head = HeadState::Invalid;
    let result = merge_trees(
        repo,
        base,
        ours,
        theirs,
        &head,
        their_name,
        base_label_prefix,
        merge_ours_oid_hex,
        merge_theirs_oid_hex,
        favor,
        diff_algorithm,
        merge_renormalize,
        ignore_all_space,
        ignore_space_change,
        ignore_space_at_eol,
        ignore_cr_at_eol,
        merge_directory_renames_mode,
        rename_options,
        None,
    )?;
    Ok(ReplayTreeMergeResult {
        index: result.index,
        has_conflicts: result.has_conflicts,
        conflict_files: result.conflict_files,
        conflict_descriptions: result.conflict_descriptions,
    })
}

enum ContentMergeResult {
    /// Clean merge: (blob oid, mode).
    Clean(ObjectId, u32),
    /// Conflict: merged content with markers.
    Conflict(Vec<u8>),
    /// Binary conflict where textual merge is not possible.
    BinaryConflict(Vec<u8>),
}

/// Try a content-level three-way merge for a single file.
fn try_content_merge(
    repo: &Repository,
    path_str: &str,
    base: &IndexEntry,
    ours: &IndexEntry,
    theirs: &IndexEntry,
    ours_label: &str,
    base_label: &str,
    theirs_label: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    merge_renormalize: bool,
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_space_at_eol: bool,
    ignore_cr_at_eol: bool,
) -> Result<ContentMergeResult> {
    let base_obj = repo.odb.read(&base.oid)?;
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    let mut base_data = base_obj.data.clone();
    let mut ours_data = ours_obj.data.clone();
    let mut theirs_data = theirs_obj.data.clone();

    let merge_behavior = resolve_path_merge_behavior(repo, path_str);

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let file_attrs = config.as_ref().map(|cfg| {
        let attrs = repo
            .work_tree
            .as_deref()
            .map(grit_lib::crlf::load_gitattributes)
            .unwrap_or_default();
        grit_lib::crlf::get_file_attrs(&attrs, path_str, cfg)
    });

    let is_attr_binary = file_attrs
        .as_ref()
        .is_some_and(|attrs| attrs.text == grit_lib::crlf::TextAttr::Unset);

    if merge_renormalize {
        base_data = renormalize_merge_blob(&base_data);
        ours_data = renormalize_merge_blob(&ours_data);
        theirs_data = renormalize_merge_blob(&theirs_data);
    }

    let base_driver_label = base_label.strip_suffix(":content").unwrap_or(base_label);
    match &merge_behavior {
        PathMergeBehavior::CustomDriver { command } => {
            let (merged, exit_code) = execute_custom_merge_driver(
                command,
                path_str,
                &base_data,
                &ours_data,
                &theirs_data,
                base_driver_label,
                ours_label,
                theirs_label,
            )?;
            if exit_code == 0 {
                let oid = repo.odb.write(ObjectKind::Blob, &merged)?;
                return Ok(ContentMergeResult::Clean(oid, ours.mode));
            }
            return Ok(ContentMergeResult::Conflict(merged));
        }
        PathMergeBehavior::CustomDriverMissing { name } => {
            bail!("merge driver '{name}' not found");
        }
        PathMergeBehavior::Default
        | PathMergeBehavior::BinaryNoMerge
        | PathMergeBehavior::Union => {}
    }

    let effective_favor = if matches!(merge_behavior, PathMergeBehavior::Union)
        && matches!(favor, MergeFavor::None)
    {
        MergeFavor::Union
    } else {
        favor
    };

    // If any is binary (by content or attribute), conflict (unless -X ours/theirs resolves it)
    if matches!(merge_behavior, PathMergeBehavior::BinaryNoMerge)
        || is_attr_binary
        || merge_file::is_binary(&base_data)
        || merge_file::is_binary(&ours_data)
        || merge_file::is_binary(&theirs_data)
    {
        match effective_favor {
            MergeFavor::Ours => {
                let oid = repo.odb.write(ObjectKind::Blob, &ours_data)?;
                return Ok(ContentMergeResult::Clean(oid, ours.mode));
            }
            MergeFavor::Theirs => {
                let oid = repo.odb.write(ObjectKind::Blob, &theirs_data)?;
                return Ok(ContentMergeResult::Clean(oid, theirs.mode));
            }
            MergeFavor::None | MergeFavor::Union => {
                return Ok(ContentMergeResult::BinaryConflict(ours_data));
            }
        }
    }

    let mut marker_warnings = Vec::new();
    let marker_size = resolve_marker_size_for_path(
        repo,
        path_str,
        ours_label,
        theirs_label,
        &mut marker_warnings,
    );
    for warning in marker_warnings {
        eprintln!("{warning}");
    }

    let conflict_style = resolve_conflict_style(repo);
    let input = MergeInput {
        base: &base_data,
        ours: &ours_data,
        theirs: &theirs_data,
        label_ours: ours_label,
        label_base: base_label,
        label_theirs: theirs_label,
        favor: effective_favor,
        style: conflict_style,
        marker_size,
        diff_algorithm: diff_algorithm.map(|s| s.to_string()),
        ignore_all_space,
        ignore_space_change,
        ignore_space_at_eol,
        ignore_cr_at_eol,
    };

    let output = merge_file::merge(&input)?;
    let mode = ours.mode; // Use ours mode by default

    if output.conflicts == 0 {
        if !merge_renormalize
            && ours_data != theirs_data
            && renormalize_merge_blob(&ours_data) == renormalize_merge_blob(&theirs_data)
        {
            let ours_text = String::from_utf8_lossy(&ours_data);
            let theirs_text = String::from_utf8_lossy(&theirs_data);
            let mut content = format!("<<<<<<< {ours_label}\n").into_bytes();
            content.extend_from_slice(ours_text.as_bytes());
            if !content.ends_with(b"\n") {
                content.push(b'\n');
            }
            content.extend_from_slice(b"=======\n");
            content.extend_from_slice(theirs_text.as_bytes());
            if !content.ends_with(b"\n") {
                content.push(b'\n');
            }
            content.extend_from_slice(format!(">>>>>>> {theirs_label}\n").as_bytes());
            return Ok(ContentMergeResult::Conflict(content));
        }
        let oid = repo.odb.write(ObjectKind::Blob, &output.content)?;
        Ok(ContentMergeResult::Clean(oid, mode))
    } else {
        Ok(ContentMergeResult::Conflict(output.content))
    }
}

/// Try content merge for add/add conflicts (empty base).
fn try_content_merge_add_add(
    repo: &Repository,
    path_str: &str,
    ours: &IndexEntry,
    theirs: &IndexEntry,
    ours_label: &str,
    theirs_label: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
    merge_renormalize: bool,
    ignore_all_space: bool,
    ignore_space_change: bool,
    ignore_space_at_eol: bool,
    ignore_cr_at_eol: bool,
) -> Result<ContentMergeResult> {
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;
    let mut ours_data = ours_obj.data.clone();
    let mut theirs_data = theirs_obj.data.clone();
    let merge_behavior = resolve_path_merge_behavior(repo, path_str);

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let file_attrs = config.as_ref().map(|cfg| {
        let attrs = repo
            .work_tree
            .as_deref()
            .map(grit_lib::crlf::load_gitattributes)
            .unwrap_or_default();
        grit_lib::crlf::get_file_attrs(&attrs, path_str, cfg)
    });

    let is_attr_binary = file_attrs
        .as_ref()
        .is_some_and(|attrs| attrs.text == grit_lib::crlf::TextAttr::Unset);

    if merge_renormalize {
        ours_data = renormalize_merge_blob(&ours_data);
        theirs_data = renormalize_merge_blob(&theirs_data);
    }

    match &merge_behavior {
        PathMergeBehavior::CustomDriver { command } => {
            let (merged, exit_code) = execute_custom_merge_driver(
                command,
                path_str,
                &[],
                &ours_data,
                &theirs_data,
                "empty tree",
                ours_label,
                theirs_label,
            )?;
            if exit_code == 0 {
                let oid = repo.odb.write(ObjectKind::Blob, &merged)?;
                return Ok(ContentMergeResult::Clean(oid, ours.mode));
            }
            return Ok(ContentMergeResult::Conflict(merged));
        }
        PathMergeBehavior::CustomDriverMissing { name } => {
            bail!("merge driver '{name}' not found");
        }
        PathMergeBehavior::Default
        | PathMergeBehavior::BinaryNoMerge
        | PathMergeBehavior::Union => {}
    }

    let effective_favor = if matches!(merge_behavior, PathMergeBehavior::Union)
        && matches!(favor, MergeFavor::None)
    {
        MergeFavor::Union
    } else {
        favor
    };

    if matches!(merge_behavior, PathMergeBehavior::BinaryNoMerge)
        || is_attr_binary
        || merge_file::is_binary(&ours_data)
        || merge_file::is_binary(&theirs_data)
    {
        return match effective_favor {
            MergeFavor::Ours => {
                let oid = repo.odb.write(ObjectKind::Blob, &ours_data)?;
                Ok(ContentMergeResult::Clean(oid, ours.mode))
            }
            MergeFavor::Theirs => {
                let oid = repo.odb.write(ObjectKind::Blob, &theirs_data)?;
                Ok(ContentMergeResult::Clean(oid, theirs.mode))
            }
            MergeFavor::None | MergeFavor::Union => {
                Ok(ContentMergeResult::BinaryConflict(ours_data))
            }
        };
    }

    let mut marker_warnings = Vec::new();
    let marker_size = resolve_marker_size_for_path(
        repo,
        path_str,
        ours_label,
        theirs_label,
        &mut marker_warnings,
    );
    for warning in marker_warnings {
        eprintln!("{warning}");
    }

    let conflict_style = resolve_conflict_style(repo);
    let input = MergeInput {
        base: &[], // empty base for add/add
        ours: &ours_data,
        theirs: &theirs_data,
        label_ours: ours_label,
        label_base: "empty tree",
        label_theirs: theirs_label,
        favor: effective_favor,
        style: conflict_style,
        marker_size,
        diff_algorithm: diff_algorithm.map(|s| s.to_string()),
        ignore_all_space,
        ignore_space_change,
        ignore_space_at_eol,
        ignore_cr_at_eol,
    };

    let output = merge_file::merge(&input)?;
    let mode = ours.mode;

    if output.conflicts == 0 {
        let oid = repo.odb.write(ObjectKind::Blob, &output.content)?;
        Ok(ContentMergeResult::Clean(oid, mode))
    } else {
        Ok(ContentMergeResult::Conflict(output.content))
    }
}

fn renormalize_merge_blob(data: &[u8]) -> Vec<u8> {
    if merge_file::is_binary(data) {
        return data.to_vec();
    }
    grit_lib::crlf::crlf_to_lf(data)
}

fn blobs_equivalent_after_renormalize(
    repo: &Repository,
    left: &IndexEntry,
    right: &IndexEntry,
) -> Result<bool> {
    let left_obj = repo.odb.read(&left.oid)?;
    let right_obj = repo.odb.read(&right.oid)?;
    Ok(renormalize_merge_blob(&left_obj.data) == renormalize_merge_blob(&right_obj.data))
}

fn resolve_conflict_style(repo: &Repository) -> ConflictStyle {
    let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) else {
        return ConflictStyle::Merge;
    };
    match config
        .get("merge.conflictstyle")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "diff3" => ConflictStyle::Diff3,
        "zdiff3" => ConflictStyle::ZealousDiff3,
        _ => ConflictStyle::Merge,
    }
}

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

fn remove_stage_zero_entry(index: &mut Index, path: &[u8]) {
    index
        .entries
        .retain(|entry| !(entry.stage() == 0 && entry.path == path));
}

fn path_has_tree_descendant(map: &HashMap<Vec<u8>, IndexEntry>, path: &[u8]) -> bool {
    map.keys()
        .any(|k| k.len() > path.len() && k.starts_with(path) && k.get(path.len()) == Some(&b'/'))
}

/// Directory/file conflicts: one side has a file at `P`, the other only has paths under `P/`.
fn apply_directory_file_conflicts(
    repo: &Repository,
    their_name: &str,
    ours_label: &str,
    ours_entries: &HashMap<Vec<u8>, IndexEntry>,
    theirs_entries: &HashMap<Vec<u8>, IndexEntry>,
    index: &mut Index,
    all_paths: &BTreeSet<Vec<u8>>,
    handled_paths: &mut BTreeSet<Vec<u8>>,
    conflict_descriptions: &mut Vec<ConflictDescription>,
    conflict_files: &mut Vec<(String, Vec<u8>)>,
    has_conflicts: &mut bool,
) -> Result<()> {
    let mut df_cases: Vec<(Vec<u8>, bool)> = Vec::new();
    for path in all_paths {
        let o = ours_entries.get(path);
        let t = theirs_entries.get(path);
        if let Some(oe) = o {
            if oe.mode != MODE_TREE && path_has_tree_descendant(theirs_entries, path) && t.is_none()
            {
                df_cases.push((path.clone(), true));
            }
        }
        if let Some(te) = t {
            if te.mode != MODE_TREE && path_has_tree_descendant(ours_entries, path) && o.is_none() {
                df_cases.push((path.clone(), false));
            }
        }
    }

    for (path, file_is_ours) in df_cases {
        handled_paths.insert(path.clone());

        let file_entry = if file_is_ours {
            ours_entries.get(&path)
        } else {
            theirs_entries.get(&path)
        }
        .ok_or_else(|| anyhow::anyhow!("directory/file conflict: missing file entry"))?;

        let branch_desc = if file_is_ours { ours_label } else { their_name };
        let new_path_str = format!("{}~{}", String::from_utf8_lossy(&path), branch_desc);
        let new_path = new_path_str.as_bytes().to_vec();

        let body = format!(
            "directory in the way of {} from {}; moving it to {} instead.",
            String::from_utf8_lossy(&path),
            branch_desc,
            new_path_str
        );
        conflict_descriptions.push(ConflictDescription {
            kind: "file/directory",
            body,
            subject_path: new_path_str.clone(),
            remerge_anchor_path: Some(String::from_utf8_lossy(&path).into_owned()),
            rename_rr_ours_dest: None,
            rename_rr_theirs_dest: None,
        });

        index.entries.retain(|e| e.path != path);
        let stage = if file_is_ours { 2u8 } else { 3u8 };
        let mut staged = file_entry.clone();
        staged.path = new_path.clone();
        stage_entry(index, &staged, stage);

        if let Ok(obj) = repo.odb.read(&file_entry.oid) {
            conflict_files.push((new_path_str, obj.data));
        }
        *has_conflicts = true;
    }

    Ok(())
}

/// Get the tree OID from a commit.
fn commit_tree(repo: &Repository, commit_oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&commit_oid)?;
    let commit = parse_commit(&obj.data)?;
    Ok(commit.tree)
}

/// Return the commit author timestamp (seconds since epoch).
///
/// Falls back to `0` when the author identity lacks a parseable timestamp.
fn commit_author_timestamp(repo: &Repository, commit_oid: ObjectId) -> Result<i64> {
    let obj = repo.odb.read(&commit_oid)?;
    let commit = parse_commit(&obj.data)?;
    let author = commit.author;
    if let Some(ts) = author
        .rsplit(' ')
        .nth(1)
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Ok(ts);
    }

    let date_text = author
        .split('>')
        .nth(1)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if date_text.is_empty() {
        return Ok(0);
    }

    let fmt = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]");
    if let Ok(fmt) = fmt {
        if let Ok(naive) = time::PrimitiveDateTime::parse(date_text, &fmt) {
            return Ok(naive.assume_utc().unix_timestamp());
        }
    }

    Ok(0)
}

/// Print a diffstat summary for merge output.
fn print_diffstat(repo: &Repository, entries: &[DiffEntry], compact: bool) {
    if entries.is_empty() {
        return;
    }

    struct StatEntry {
        path: String,
        insertions: usize,
        deletions: usize,
        is_new: bool,
        is_deleted: bool,
        new_mode: Option<u32>,
    }

    let mut stats: Vec<StatEntry> = Vec::new();
    let mut total_ins = 0usize;
    let mut total_del = 0usize;

    for entry in entries {
        let path = entry
            .new_path
            .as_deref()
            .or(entry.old_path.as_deref())
            .unwrap_or("unknown");
        let is_new = entry.old_oid == grit_lib::diff::zero_oid();
        let is_deleted = entry.new_oid == grit_lib::diff::zero_oid();

        // Read blob contents to compute changes
        let old_content = if !is_new {
            repo.odb
                .read(&entry.old_oid)
                .ok()
                .map(|o| String::from_utf8_lossy(&o.data).to_string())
        } else {
            None
        };
        let new_content = if !is_deleted {
            repo.odb
                .read(&entry.new_oid)
                .ok()
                .map(|o| String::from_utf8_lossy(&o.data).to_string())
        } else {
            None
        };

        let (ins, del) = count_changes(
            old_content.as_deref().unwrap_or(""),
            new_content.as_deref().unwrap_or(""),
        );

        total_ins += ins;
        total_del += del;

        let mode_num = u32::from_str_radix(&entry.new_mode, 8).unwrap_or(0o100644);
        stats.push(StatEntry {
            path: path.to_owned(),
            insertions: ins,
            deletions: del,
            is_new,
            is_deleted,
            new_mode: if is_new { Some(mode_num) } else { None },
        });
    }

    // Build display names with compact annotations
    let display_names: Vec<String> = stats
        .iter()
        .map(|s| {
            if compact {
                let mut name = s.path.clone();
                if s.is_new {
                    name.push_str(" (new)");
                } else if s.is_deleted {
                    name.push_str(" (gone)");
                }
                // Could also add mode changes here
                name
            } else {
                s.path.clone()
            }
        })
        .collect();

    let max_path_len = display_names.iter().map(|s| s.len()).max().unwrap_or(0);
    let max_change = stats
        .iter()
        .map(|s| s.insertions + s.deletions)
        .max()
        .unwrap_or(0);
    let count_width = if max_change == 0 {
        1
    } else {
        format!("{}", max_change).len()
    };

    for (i, s) in stats.iter().enumerate() {
        let total = s.insertions + s.deletions;
        let plus = "+".repeat(s.insertions.min(50));
        let minus = "-".repeat(s.deletions.min(50));
        println!(
            " {:<width$} | {:>cw$} {}{}",
            display_names[i],
            total,
            plus,
            minus,
            width = max_path_len,
            cw = count_width
        );
    }

    // Summary line
    let files_changed = stats.len();
    let mut parts = Vec::new();
    parts.push(format!(
        "{} file{} changed",
        files_changed,
        if files_changed != 1 { "s" } else { "" }
    ));
    if total_ins > 0 {
        parts.push(format!(
            "{} insertion{}",
            total_ins,
            if total_ins != 1 { "s(+)" } else { "(+)" }
        ));
    }
    if total_del > 0 {
        parts.push(format!(
            "{} deletion{}",
            total_del,
            if total_del != 1 { "s(-)" } else { "(-)" }
        ));
    }
    println!(" {}", parts.join(", "));

    // Show create/delete mode notices (not needed in compact mode)
    if !compact {
        for s in &stats {
            if s.is_new {
                if let Some(mode) = s.new_mode {
                    println!(" create mode {:06o} {}", mode, s.path);
                }
            }
            if s.is_deleted {
                println!(" delete mode 100644 {}", s.path);
            }
        }
    }
}

/// True if `dir` exists and contains only `.` and `..` (safe to replace with a submodule gitlink).
fn is_empty_dir_for_submodule_placeholder(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        if name != "." && name != ".." {
            return false;
        }
    }
    true
}

/// Refresh cached stat data for every stage-0 index entry from the work tree.
///
/// Tree-built indexes start with zeroed stat fields; without refreshing,
/// `git diff-files` falsely reports every tracked file as modified.
fn refresh_index_stat_cache_from_worktree(repo: &Repository, index: &mut Index) -> Result<()> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(());
    };
    for entry in &mut index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let abs = work_tree.join(path_str.as_ref());
        if let Ok(meta) = fs::symlink_metadata(&abs) {
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.size = meta.len() as u32;
        }
    }
    Ok(())
}

/// Recursively flatten a tree into index entries.
fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            let sub = tree_to_index_entries(repo, &te.oid, &path)?;
            result.extend(sub);
        } else {
            let path_bytes = path.into_bytes();
            result.push(IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: te.mode,
                uid: 0,
                gid: 0,
                size: 0,
                oid: te.oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes,
            });
        }
    }
    Ok(result)
}

fn tree_to_map(entries: Vec<IndexEntry>) -> HashMap<Vec<u8>, IndexEntry> {
    let mut out = HashMap::new();
    for e in entries {
        out.insert(e.path.clone(), e);
    }
    out
}

/// Resolve a merge target (branch name or commit-ish).
fn resolve_merge_target(repo: &Repository, spec: &str) -> Result<ObjectId> {
    use grit_lib::rev_parse::resolve_revision;
    if let Some(oid) = grit_lib::rev_parse::resolve_at_minus_to_oid(repo, spec)? {
        Ok(oid)
    } else {
        resolve_revision(repo, spec).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

fn read_fetch_head_merge_oids(repo: &Repository) -> Result<Vec<String>> {
    let fetch_head_path = repo.git_dir.join("FETCH_HEAD");
    let content = fs::read_to_string(&fetch_head_path)
        .with_context(|| "FETCH_HEAD: object not found: FETCH_HEAD".to_string())?;

    let mut oids = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let Some(oid) = parts.next() else {
            continue;
        };
        let not_for_merge = parts.next().is_some_and(|value| value == "not-for-merge");
        if not_for_merge {
            continue;
        }
        if oid.len() == 40 && oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            oids.push(oid.to_owned());
        }
    }

    if oids.is_empty() {
        bail!("FETCH_HEAD: object not found: FETCH_HEAD");
    }
    Ok(oids)
}

#[derive(Clone)]
struct AutoStashEntry {
    path: String,
    content: Vec<u8>,
}

fn capture_dirty_tracked_entries(repo: &Repository) -> Result<Vec<AutoStashEntry>> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(Vec::new());
    };
    let index = repo.load_index()?;
    let mut entries = Vec::new();
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path = String::from_utf8_lossy(&entry.path).to_string();
        let abs = work_tree.join(&path);
        if fs::symlink_metadata(&abs).is_err() {
            continue;
        }
        if is_worktree_entry_dirty(repo, entry, &abs)? {
            if let Ok(content) = fs::read(&abs) {
                entries.push(AutoStashEntry { path, content });
            }
        }
    }
    Ok(entries)
}

fn apply_autostash_entries(repo: &Repository, entries: &[AutoStashEntry]) -> Result<()> {
    let Some(work_tree) = repo.work_tree.as_deref() else {
        return Ok(());
    };
    for entry in entries {
        let abs = work_tree.join(&entry.path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs, &entry.content)?;
    }
    Ok(())
}

/// Build the default merge commit message.
/// Append Signed-off-by trailer to a message if not already present.
fn append_signoff(msg: &str, name: &str, email: &str) -> String {
    let trailer = format!("Signed-off-by: {} <{}>", name, email);
    if msg.contains(&trailer) {
        return msg.to_string();
    }
    let trimmed = msg.trim_end();
    format!("{}\n\n{}\n", trimmed, trailer)
}

/// UTF-8 merge message plus optional raw bytes and `encoding` header for the commit object.
struct MergeCommitMessage {
    message: String,
    encoding: Option<String>,
    raw_message: Option<Vec<u8>>,
}

fn read_merge_message_from_file(path: &Path, config: &ConfigSet) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("could not read merge message file: {path:?}"))?;
    if let Ok(s) = String::from_utf8(bytes.clone()) {
        return Ok(s);
    }
    let enc_name = config
        .get("i18n.commitEncoding")
        .or_else(|| config.get("i18n.commitencoding"));
    Ok(crate::git_commit_encoding::decode_bytes(
        enc_name.as_deref(),
        &bytes,
    ))
}

fn finalize_merge_commit_message(msg: String, config: &ConfigSet) -> MergeCommitMessage {
    let commit_enc = config
        .get("i18n.commitEncoding")
        .or_else(|| config.get("i18n.commitencoding"));
    let is_utf8 = match commit_enc.as_deref() {
        None => true,
        Some(e) => e.eq_ignore_ascii_case("utf-8") || e.eq_ignore_ascii_case("utf8"),
    };
    if is_utf8 {
        return MergeCommitMessage {
            message: msg,
            encoding: None,
            raw_message: None,
        };
    }
    let Some(label) = commit_enc else {
        return MergeCommitMessage {
            message: msg,
            encoding: None,
            raw_message: None,
        };
    };
    let Some(raw) = crate::git_commit_encoding::encode_unicode(&label, &msg) else {
        return MergeCommitMessage {
            message: msg,
            encoding: None,
            raw_message: None,
        };
    };
    MergeCommitMessage {
        message: msg,
        encoding: Some(label),
        raw_message: Some(raw),
    }
}

fn build_merge_message(
    head: &HeadState,
    branch_name: &str,
    custom: Option<&str>,
    repo: &Repository,
) -> String {
    if let Some(msg) = custom {
        return ensure_trailing_newline(msg);
    }
    // For @{-N} (and @{-N}<suffix>) specs, use the resolved previous branch
    // name in the default merge message, matching git's behavior.
    let display_branch = if branch_name.starts_with("@{-") {
        if let Some(close) = branch_name.find('}') {
            let token = &branch_name[..=close];
            match grit_lib::rev_parse::expand_at_minus_to_branch_name(repo, token) {
                Ok(Some(name)) => name,
                _ => branch_name.to_string(),
            }
        } else {
            branch_name.to_string()
        }
    } else {
        branch_name.to_string()
    };
    // Determine if the merge target is a tag, branch, or commit
    let kind = if resolve_ref(&repo.git_dir, &format!("refs/tags/{display_branch}")).is_ok() {
        "tag"
    } else if resolve_ref(&repo.git_dir, &format!("refs/remotes/{display_branch}")).is_ok() {
        "remote-tracking branch"
    } else {
        "branch"
    };
    let base_msg = format!("Merge {kind} '{display_branch}'");
    // Append "into <branch>" if not merging into main/master
    let msg = if let Some(name) = head.branch_name() {
        if name != "main" && name != "master" {
            format!("{base_msg} into {name}")
        } else {
            base_msg
        }
    } else {
        base_msg
    };
    ensure_trailing_newline(&msg)
}

/// Update HEAD to point to the given commit.
fn update_head(git_dir: &Path, head: &HeadState, commit_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
        }
    }
    Ok(())
}

/// Remove files from working tree that existed before but are no longer in the merged index.
fn remove_deleted_files(
    work_tree: &Path,
    old_entries: &HashMap<Vec<u8>, IndexEntry>,
    new_index: &Index,
) -> Result<()> {
    let new_paths: std::collections::HashSet<&[u8]> = new_index
        .entries
        .iter()
        .map(|e| e.path.as_slice())
        .collect();
    for (path, old_entry) in old_entries {
        if new_paths.contains(path.as_slice()) {
            continue;
        }
        let has_nested_under = new_index.entries.iter().any(|e| {
            e.path.starts_with(path)
                && e.path.len() > path.len()
                && e.path.get(path.len()) == Some(&b'/')
        });
        // Submodule removed from the superproject: keep the on-disk work tree.
        if old_entry.mode == MODE_GITLINK && !has_nested_under {
            continue;
        }
        let path_str = String::from_utf8_lossy(path);
        let abs = work_tree.join(path_str.as_ref());
        if abs.exists() || fs::symlink_metadata(&abs).is_ok() {
            if abs.is_dir() {
                let _ = fs::remove_dir_all(&abs);
            } else {
                let _ = fs::remove_file(&abs);
            }
            remove_empty_parent_dirs_merge(work_tree, &abs);
        }
    }
    Ok(())
}

fn remove_empty_parent_dirs_merge(work_tree: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

/// Checkout index entries to working tree.
fn checkout_entries(
    repo: &Repository,
    work_tree: &Path,
    index: &Index,
    old_entries: Option<&HashMap<Vec<u8>, IndexEntry>>,
) -> Result<()> {
    // Load gitattributes and config for CRLF conversion
    let mut attr_rules = grit_lib::crlf::load_gitattributes(work_tree);
    if index.get(b".gitattributes", 0).is_some() {
        let from_index = grit_lib::crlf::load_gitattributes_from_index(index, &repo.odb);
        if !from_index.is_empty() {
            attr_rules = from_index;
        }
    }
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let conv = config
        .as_ref()
        .map(grit_lib::crlf::ConversionConfig::from_config);

    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        if old_entries.is_some_and(|old| {
            old.get(&entry.path)
                .is_some_and(|previous| previous.oid == entry.oid && previous.mode == entry.mode)
        }) {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        // Directory/file conflicts: a tracked file may occupy a path that the merge
        // result needs as a directory (e.g. `path/file` while `path` was a file).
        let mut cur = abs_path.parent();
        while let Some(dir) = cur {
            if dir == work_tree {
                break;
            }
            if dir.exists() && !dir.is_dir() {
                let _ = fs::remove_file(dir);
            }
            cur = dir.parent();
        }

        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Submodule entries (gitlinks): materialize an empty directory in the
        // superproject (Git does not check out submodule contents on merge).
        if entry.mode == 0o160000 {
            if abs_path.is_file() || abs_path.is_symlink() {
                let _ = fs::remove_file(&abs_path);
            } else if abs_path.is_dir() && abs_path.join(".git").exists() {
                continue;
            } else if abs_path.is_dir() {
                let _ = fs::remove_dir_all(&abs_path);
            }
            let _ = fs::create_dir_all(&abs_path);
            continue;
        }

        let obj = repo.odb.read(&entry.oid)?;
        if obj.kind != ObjectKind::Blob {
            continue;
        }

        if abs_path.is_dir() {
            fs::remove_dir_all(&abs_path)?;
        }

        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if abs_path.exists() || abs_path.is_symlink() {
                let _ = fs::remove_file(&abs_path);
            }
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            // Apply CRLF conversion if configured
            let data = if let (Some(ref config), Some(ref conv)) = (&config, &conv) {
                let file_attrs = grit_lib::crlf::get_file_attrs(&attr_rules, &path_str, config);
                grit_lib::crlf::convert_to_worktree(
                    &obj.data,
                    &path_str,
                    conv,
                    &file_attrs,
                    None,
                    None,
                )
                .map_err(|e| anyhow::anyhow!("smudge filter failed for {path_str}: {e}"))?
            } else {
                obj.data.clone()
            };
            fs::write(&abs_path, &data)?;
            if entry.mode == MODE_EXECUTABLE {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&abs_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&abs_path, perms)?;
            }
        }
    }
    Ok(())
}

/// Resolve author/committer identity from env and config.
fn resolve_ident(config: &ConfigSet, kind: &str, now: OffsetDateTime) -> Result<String> {
    let name_var = if kind == "author" {
        "GIT_AUTHOR_NAME"
    } else {
        "GIT_COMMITTER_NAME"
    };
    let email_var = if kind == "author" {
        "GIT_AUTHOR_EMAIL"
    } else {
        "GIT_COMMITTER_EMAIL"
    };
    let date_var = if kind == "author" {
        "GIT_AUTHOR_DATE"
    } else {
        "GIT_COMMITTER_DATE"
    };

    let name = std::env::var(name_var)
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());

    let email = std::env::var(email_var)
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    let timestamp = std::env::var(date_var)
        .map(|d| parse_date_to_git_ts(&d).unwrap_or(d))
        .unwrap_or_else(|_| {
            let epoch = now.unix_timestamp();
            let offset = now.offset();
            let hours = offset.whole_hours();
            let minutes = offset.minutes_past_hour().unsigned_abs();
            format!("{epoch} {hours:+03}{minutes:02}")
        });

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Parse date string to git timestamp format (epoch + offset).
fn parse_date_to_git_ts(date_str: &str) -> Option<String> {
    let trimmed = date_str.trim();
    let parts: Vec<&str> = trimmed.rsplitn(2, ' ').collect();
    if parts.len() == 2 {
        let maybe_epoch = parts[1];
        if maybe_epoch.chars().all(|c| c.is_ascii_digit()) {
            return None; // already in epoch format
        }
        let tz = parts[0];
        let datetime = parts[1];
        let tz_bytes = tz.as_bytes();
        if tz_bytes.len() >= 5 {
            let sign: i64 = if tz_bytes[0] == b'-' { -1 } else { 1 };
            let h: i64 = tz[1..3].parse().unwrap_or(0);
            let m: i64 = tz[3..5].parse().unwrap_or(0);
            let tz_secs = sign * (h * 3600 + m * 60);
            if let Ok(offset) = time::UtcOffset::from_whole_seconds(tz_secs as i32) {
                let fmt = time::format_description::parse(
                    "[year]-[month]-[day] [hour]:[minute]:[second]",
                )
                .ok()?;
                if let Ok(naive) = time::PrimitiveDateTime::parse(datetime, &fmt) {
                    let dt = naive.assume_offset(offset);
                    return Some(format!("{} {}", dt.unix_timestamp(), tz));
                }
            }
        }
    }
    None
}

/// Apply cleanup mode to a commit message.
fn cleanup_message(msg: &str, mode: &str) -> String {
    match mode {
        "verbatim" => {
            // Keep message exactly as-is
            msg.to_string()
        }
        "whitespace" => {
            // Strip trailing whitespace from each line, leading and trailing blank lines
            let lines: Vec<&str> = msg.lines().collect();
            let mut result: Vec<String> = lines.iter().map(|l| l.trim_end().to_string()).collect();
            // Remove leading empty lines
            while result.first().is_some_and(|l| l.is_empty()) {
                result.remove(0);
            }
            // Remove trailing empty lines
            while result.last().is_some_and(|l| l.is_empty()) {
                result.pop();
            }
            if result.is_empty() {
                String::new()
            } else {
                result.join("\n") + "\n"
            }
        }
        "strip" | "default" => {
            // Strip comments (lines starting with #) and trailing whitespace
            let lines: Vec<&str> = msg.lines().collect();
            let mut result: Vec<String> = lines
                .iter()
                .filter(|l| !l.starts_with('#'))
                .map(|l| l.trim_end().to_string())
                .collect();
            // Remove leading empty lines
            while result.first().is_some_and(|l| l.is_empty()) {
                result.remove(0);
            }
            // Remove trailing empty lines
            while result.last().is_some_and(|l| l.is_empty()) {
                result.pop();
            }
            if result.is_empty() {
                String::new()
            } else {
                result.join("\n") + "\n"
            }
        }
        "scissors" => {
            // Strip everything from the scissors line onward.
            // A scissors line starts at column 0 (not indented).
            let mut result_lines: Vec<&str> = Vec::new();
            for line in msg.lines() {
                if line.starts_with("# ------------------------ >8 ------------------------") {
                    break;
                }
                result_lines.push(line);
            }
            // Strip trailing whitespace from lines, leading and trailing blank lines
            let mut result: Vec<String> = result_lines
                .iter()
                .map(|l| l.trim_end().to_string())
                .collect();
            // Remove leading empty lines
            while result.first().is_some_and(|l| l.is_empty()) {
                result.remove(0);
            }
            // Remove trailing empty lines
            while result.last().is_some_and(|l| l.is_empty()) {
                result.pop();
            }
            if result.is_empty() {
                String::new()
            } else {
                result.join("\n") + "\n"
            }
        }
        _ => {
            // Unknown mode: treat as default
            cleanup_message(msg, "strip")
        }
    }
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}
