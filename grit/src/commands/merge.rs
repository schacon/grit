//! `grit merge` — join two or more development histories together.
//!
//! Implements fast-forward, three-way merge with conflict handling,
//! `--squash`, `--no-ff`, `--ff-only`, `--abort`, and `--continue`.

use crate::commands::git_passthrough;
use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::diff::{count_changes, detect_renames, diff_trees, DiffEntry, DiffStatus};
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
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

/// Arguments for `grit merge`.
#[derive(Debug, ClapArgs)]
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
    #[arg(short = 's', long = "strategy")]
    pub strategy: Option<String>,

    /// Strategy-specific option (e.g. ours, theirs).
    #[arg(short = 'X', long = "strategy-option")]
    pub strategy_option: Vec<String>,

    /// Suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

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
    if args.strategy.as_deref() == Some("help") {
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
    if repo.git_dir.join("rr-cache").is_dir() {
        return passthrough_current_merge_invocation();
    }
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

    if args.squash && args.no_ff {
        bail!("fatal: You cannot combine --squash with --no-ff.");
    }
    if args.squash && args.commit {
        bail!("fatal: You cannot combine --squash with --commit.");
    }

    // Validate --strategy: accept known names, warn on unsupported ones.
    if let Some(ref strat) = args.strategy {
        match strat.as_str() {
            "recursive" | "ort" | "resolve" => {
                // These are compatible with our three-way merge implementation.
            }
            "octopus" => {
                // Octopus is handled separately when multiple commits are given.
            }
            "ours" | "theirs" | "subtree" => {
                // "ours" strategy: keep our tree, just make a merge commit.
                // "theirs" strategy: keep their tree, just make a merge commit.
                // "subtree" strategy: variant of recursive.
                // We handle this specially below.
            }
            // "help" is handled above before the commit check
            other => {
                bail!("Could not find merge strategy '{}'", other);
            }
        }
    }

    // Parse -X strategy options
    let mut favor = MergeFavor::None;
    let mut diff_algorithm: Option<String> = None;
    for xopt in &args.strategy_option {
        if let Some(algo) = xopt.strip_prefix("diff-algorithm=") {
            diff_algorithm = Some(algo.to_string());
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
        return do_octopus_merge(&repo, &head, head_oid, &args, favor, diff_algorithm.as_deref());
    }

    // Resolve merge target
    let merge_oid = resolve_merge_target(&repo, &args.commits[0])?;

    // Handle -s ours: keep our tree, just create merge commit
    if args.strategy.as_deref() == Some("ours") {
        // Even with -s ours, if merge target is ancestor of HEAD, already up-to-date
        if merge_oid == head_oid || is_ancestor(&repo, merge_oid, head_oid)? {
            if !args.quiet {
                eprintln!("Already up to date.");
            }
            return Ok(());
        }
        return do_strategy_ours(&repo, &head, head_oid, merge_oid, &args);
    }

    // Handle -s theirs: keep their tree, just create merge commit
    if args.strategy.as_deref() == Some("theirs") {
        if merge_oid == head_oid || is_ancestor(&repo, merge_oid, head_oid)? {
            if !args.quiet {
                eprintln!("Already up to date.");
            }
            return Ok(());
        }
        return do_strategy_theirs(&repo, &head, head_oid, merge_oid, &args);
    }

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
            // Force a merge commit even though we could fast-forward
            return do_real_merge(&repo, &head, head_oid, merge_oid, &args, favor, diff_algorithm.as_deref());
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

    do_real_merge(&repo, &head, head_oid, merge_oid, &args, favor, diff_algorithm.as_deref())
}

/// Handle merge when HEAD is unborn — just set HEAD to merge target.
fn merge_unborn(repo: &Repository, head: &HeadState, args: &Args) -> Result<()> {
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
        checkout_entries(repo, wt, &index)?;
    }
    index.write(&repo.index_path())?;

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

    update_head(&repo.git_dir, head, &merge_oid)?;

    // Update index and working tree
    let commit_obj = repo.odb.read(&merge_oid)?;
    let commit = parse_commit(&commit_obj.data)?;
    let entries = tree_to_index_entries(repo, &commit.tree, "")?;
    let mut new_index = Index::new();
    new_index.entries = entries;
    new_index.sort();

    if let Some(ref wt) = repo.work_tree {
        // Remove files that existed in old HEAD but not in new
        let old_tree = commit_tree(repo, head_oid)?;
        let old_entries = tree_to_map(tree_to_index_entries(repo, &old_tree, "")?);
        remove_deleted_files(wt, &old_entries, &new_index)?;
        checkout_entries(repo, wt, &new_index)?;
    }
    new_index.write(&repo.index_path())?;

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
    Ok(())
}

/// Perform a real three-way merge.
/// Create a virtual merge base by recursively merging multiple merge bases.
/// This handles criss-cross merge situations where there are multiple LCA commits.
fn create_virtual_merge_base(
    repo: &Repository,
    bases: &[ObjectId],
    favor: MergeFavor,
) -> Result<ObjectId> {
    if bases.len() == 1 {
        return Ok(bases[0]);
    }

    // Recursively merge bases pairwise
    let mut current = bases[0];
    for &next in &bases[1..] {
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
            create_virtual_merge_base(repo, &sub_bases, favor)?
        } else {
            sub_bases[0]
        };

        // Merge current and next using sub_base_oid as base
        let base_tree = commit_tree(repo, sub_base_oid)?;
        let ours_tree = commit_tree(repo, current)?;
        let theirs_tree = commit_tree(repo, next)?;

        let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
        let ours_entries = tree_to_map(tree_to_index_entries(repo, &ours_tree, "")?);
        let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

        // Create a dummy head state for merge_trees
        let head = HeadState::Detached { oid: current };
        let merge_result = merge_trees(
            repo,
            &base_entries,
            &ours_entries,
            &theirs_entries,
            &head,
            "virtual",
            favor,
            None,
        )?;

        // Build a tree from the merged index (use stage-0 entries, or for conflicts pick ours)
        let mut final_entries: Vec<IndexEntry> = Vec::new();
        let mut seen_paths: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
        // First collect stage 0 entries
        for entry in &merge_result.index.entries {
            if entry.stage() == 0 && seen_paths.insert(entry.path.clone()) {
                final_entries.push(entry.clone());
            }
        }
        // For conflicted paths (no stage 0), pick stage 2 (ours)
        for entry in &merge_result.index.entries {
            if entry.stage() == 2 && seen_paths.insert(entry.path.clone()) {
                let mut e = entry.clone();
                e.flags &= !0x3000; // Clear stage bits → stage 0
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

fn do_real_merge(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
) -> Result<()> {
    // Find merge base(s)
    let bases = grit_lib::merge_base::merge_bases_first_vs_rest(repo, head_oid, &[merge_oid])?;
    if bases.is_empty() {
        bail!("refusing to merge unrelated histories");
    }
    // If multiple merge bases (criss-cross):
    // - resolve strategy: fail (doesn't support virtual merge bases)
    // - recursive/ort: create a virtual merge base
    let base_oid = if bases.len() > 1 {
        if args.strategy.as_deref() == Some("resolve") {
            bail!("merge: warning: multiple common ancestors found");
        }
        create_virtual_merge_base(repo, &bases, favor)?
    } else {
        bases[0]
    };

    // Get trees
    let base_tree = commit_tree(repo, base_oid)?;
    let ours_tree = commit_tree(repo, head_oid)?;
    let theirs_tree = commit_tree(repo, merge_oid)?;

    // Flatten trees to path→entry maps
    let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
    let ours_entries = tree_to_map(tree_to_index_entries(repo, &ours_tree, "")?);
    let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

    // Save ORIG_HEAD
    fs::write(
        repo.git_dir.join("ORIG_HEAD"),
        format!("{}\n", head_oid.to_hex()),
    )?;

    // Merge trees
    let merge_result = merge_trees(
        repo,
        &base_entries,
        &ours_entries,
        &theirs_entries,
        head,
        &args.commits[0],
        favor,
        diff_algorithm,
    )?;

    // Write index
    merge_result.index.write(&repo.index_path())?;

    // Update working tree
    if let Some(ref wt) = repo.work_tree {
        // Remove files that were in ours but are no longer in the merged index
        remove_deleted_files(wt, &ours_entries, &merge_result.index)?;
        checkout_entries(repo, wt, &merge_result.index)?;
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
                grit_lib::crlf::convert_to_worktree(content, path, &conv, &file_attrs, None)
            } else {
                content.clone()
            };
            fs::write(&abs, &output)?;
        }
    }

    if merge_result.has_conflicts {
        if args.squash {
            // For squash + conflict: write SQUASH_MSG with conflict info, no MERGE_HEAD
            let mut msg = build_squash_msg(repo, head_oid, &[merge_oid])?;
            // Append conflict info
            msg.push_str("# Conflicts:\n");
            for (_ctype, cpath) in &merge_result.conflict_descriptions {
                msg.push_str(&format!("#\t{cpath}\n"));
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
        for (ctype, cpath) in &merge_result.conflict_descriptions {
            if ctype == "rename/delete" || ctype == "modify/delete" {
                println!("CONFLICT ({ctype}): {cpath}");
            } else {
                println!("CONFLICT ({ctype}): Merge conflict in {cpath}");
            }
        }
        println!("Automatic merge failed; fix conflicts and then commit the result.");
        eprintln!("Automatic merge failed; fix conflicts and then commit the result.");
        std::process::exit(1);
    }

    if args.squash {
        return do_squash_from_merge(repo, &merge_result.index, head, head_oid, merge_oid, args);
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
        return Ok(());
    }

    // Create merge commit
    let tree_oid = write_tree_from_index(&repo.odb, &merge_result.index, "")?;
    let effective_custom_msg = if let Some(ref file_path) = args.file {
        Some(fs::read_to_string(file_path)
            .with_context(|| format!("could not read merge message file: {file_path}"))?)
    } else {
        args.message.clone()
    };
    let mut msg = build_merge_message(head, &args.commits[0], effective_custom_msg.as_deref(), repo);

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

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
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
        eprintln!("[{branch} {short}] {first_line}");

        // Print strategy message (to stdout, as git does)
        let strategy_name = args.strategy.as_deref().unwrap_or("ort");
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

    // If only one merge target remains after filtering, delegate to single merge
    if merge_oids.len() == 1 {
        let merge_oid = merge_oids[0];
        if args.no_ff && !args.ff_only {
            return do_real_merge(repo, head, head_oid, merge_oid, args, favor, diff_algorithm);
        }
        if is_ancestor(repo, head_oid, merge_oid)? {
            return do_fast_forward(repo, head, head_oid, merge_oid, args);
        }
        return do_real_merge(repo, head, head_oid, merge_oid, args, favor, diff_algorithm);
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
        if bases.is_empty() {
            bail!("refusing to merge unrelated histories");
        }
        let base_oid = bases[0];
        let base_tree = commit_tree(repo, base_oid)?;
        let theirs_tree = commit_tree(repo, *merge_oid)?;

        let base_entries = tree_to_map(tree_to_index_entries(repo, &base_tree, "")?);
        let ours_entries = tree_to_map(current_tree_entries);
        let theirs_entries = tree_to_map(tree_to_index_entries(repo, &theirs_tree, "")?);

        let merge_result = merge_trees(
            repo,
            &base_entries,
            &ours_entries,
            &theirs_entries,
            head,
            &args.commits[i],
            favor,
            diff_algorithm,
        )?;

        if merge_result.has_conflicts {
            // Octopus strategy cannot handle conflicts - restore original state
            let orig_tree = commit_tree(repo, head_oid)?;
            let orig_entries = tree_to_index_entries(repo, &orig_tree, "")?;
            let mut orig_index = Index::new();
            orig_index.entries = orig_entries;
            orig_index.sort();
            orig_index.write(&repo.index_path())?;
            if let Some(ref wt) = repo.work_tree {
                checkout_entries(repo, wt, &orig_index)?;
            }
            let _ = fs::remove_file(repo.git_dir.join("MERGE_HEAD"));
            let _ = fs::remove_file(repo.git_dir.join("MERGE_MSG"));
            let _ = fs::remove_file(repo.git_dir.join("MERGE_MODE"));
            eprintln!("Merge with strategy octopus failed.");
            println!("Should not be doing an octopus.");
            eprintln!("fatal: merge program failed");
            std::process::exit(2);
        }

        // Advance current_tree_entries to the merged result
        current_tree_entries = merge_result.index.entries;
    }

    // All merges succeeded — build the octopus merge commit
    let mut final_index = Index::new();
    final_index.entries = current_tree_entries;
    final_index.sort();
    final_index.write(&repo.index_path())?;

    if let Some(ref wt) = repo.work_tree {
        checkout_entries(repo, wt, &final_index)?;
    }

    if args.squash {
        let msg = build_squash_msg(repo, head_oid, &merge_oids)?;
        fs::write(repo.git_dir.join("SQUASH_MSG"), &msg)?;
        if !args.quiet {
            eprintln!("Squash commit -- not updating HEAD");
        }
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
        return Ok(());
    }

    let tree_oid = write_tree_from_index(&repo.odb, &final_index, "")?;
    let msg = build_octopus_merge_message(head, &merge_names, args.message.as_deref(), repo);

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let now = OffsetDateTime::now_utc();
    let author = resolve_ident(&config, "author", now)?;
    let committer = resolve_ident(&config, "committer", now)?;

    let mut parents = vec![head_oid];
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
        eprintln!("[{branch} {short}] {first_line}");
    }

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
        eprintln!("[{branch} {short}] {first_line}");
    }

    Ok(())
}

fn do_strategy_theirs(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
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
        checkout_entries(repo, wt, &new_index)?;
    }
    new_index.write(&repo.index_path())?;

    if !args.quiet {
        let short = &commit_oid.to_hex()[..7];
        let branch = head.branch_name().unwrap_or("HEAD");
        let first_line = commit_data.message.lines().next().unwrap_or("");
        eprintln!("[{branch} {short}] {first_line}");
    }

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
        checkout_entries(repo, wt, &new_index)?;
    }
    new_index.write(&repo.index_path())?;

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
    Ok(())
}

/// Squash from a three-way merge result.
fn do_squash_from_merge(
    repo: &Repository,
    index: &Index,
    _head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
) -> Result<()> {
    index.write(&repo.index_path())?;

    let msg = build_squash_msg(repo, head_oid, &[merge_oid])?;
    fs::write(repo.git_dir.join("SQUASH_MSG"), &msg)?;

    if !args.quiet {
        eprintln!("Squash commit -- not updating HEAD");
    }
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
        checkout_entries(&repo, wt, &index)?;
    }
    index.write(&repo.index_path())?;

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
    let index = match Index::load(&repo.index_path()) {
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
    eprintln!("[{branch} {short}] {first_line}");

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

struct MergeResult {
    index: Index,
    has_conflicts: bool,
    /// Files with conflict markers: (path, content).
    conflict_files: Vec<(String, Vec<u8>)>,
    /// Conflict descriptions for output: (conflict_type, path).
    /// e.g. ("content", "file.txt") or ("modify/delete", "file.txt")
    conflict_descriptions: Vec<(String, String)>,
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
) -> (HashMap<Vec<u8>, Vec<u8>>, HashMap<Vec<u8>, Vec<u8>>) {
    let threshold = 50u32;
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

/// Perform tree-level three-way merge.
fn merge_trees(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    _head: &HeadState,
    their_name: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
) -> Result<MergeResult> {
    // Detect renames on each side
    let (ours_renames, theirs_renames) = detect_merge_renames(repo, base, ours, theirs);

    // Track which paths are handled via rename logic so we don't double-process
    let mut handled_paths: BTreeSet<Vec<u8>> = BTreeSet::new();

    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut index = Index::new();
    let mut has_conflicts = false;
    let mut conflict_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut conflict_descriptions: Vec<(String, String)> = Vec::new();

    let ours_label = "HEAD";

    // First pass: handle rename cases
    // Case 1: ours renamed base_path → ours_new_path; theirs may have modified base_path
    for (base_path, ours_new_path) in &ours_renames {
        handled_paths.insert(base_path.clone());
        // The new path on ours side is handled here too (don't treat as add/add)
        handled_paths.insert(ours_new_path.clone());

        let be = base.get(base_path);
        let oe = ours.get(ours_new_path); // The renamed file in ours
        let te = theirs.get(base_path); // Theirs' version at original path

        if let (Some(be), Some(oe)) = (be, oe) {
            if let Some(te) = te {
                // Theirs also has the file at the old path — merge content at new path
                if be.oid == te.oid && be.mode == te.mode {
                    // Theirs didn't modify — just use ours (renamed version)
                    index.entries.push(oe.clone());
                } else if oe.oid == te.oid {
                    // Both made same change
                    index.entries.push(oe.clone());
                } else {
                    // Both modified — try content merge at new path
                    let path_str = String::from_utf8_lossy(ours_new_path).to_string();
                    match try_content_merge(repo, be, oe, te, ours_label, their_name, favor, diff_algorithm)? {
                        ContentMergeResult::Clean(merged_oid, mode) => {
                            let mut entry = oe.clone();
                            entry.oid = merged_oid;
                            entry.mode = mode;
                            index.entries.push(entry);
                        }
                        ContentMergeResult::Conflict(content) => {
                            has_conflicts = true;
                            let mut be_at_new = be.clone();
                            be_at_new.path = ours_new_path.clone();
                            stage_entry(&mut index, &be_at_new, 1);
                            stage_entry(&mut index, oe, 2);
                            let mut te_at_new = te.clone();
                            te_at_new.path = ours_new_path.clone();
                            stage_entry(&mut index, &te_at_new, 3);
                            conflict_descriptions.push(("content".to_string(), path_str.clone()));
                            conflict_files.push((path_str, content));
                        }
                    }
                }
            } else {
                // Theirs deleted the original — ours renamed it → rename/delete conflict
                has_conflicts = true;
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
                conflict_descriptions.push(("rename/delete".to_string(), format!(
                    "{base_path_str} deleted in {their_name} and renamed to {new_path_str} in {ours_label}. Version {ours_label} of {new_path_str} left in tree."
                )));
            }

            // If theirs also has a NEW file at ours_new_path (add/add at rename target)
            if let Some(te_at_new) = theirs.get(ours_new_path) {
                if !base.contains_key(ours_new_path) {
                    // Theirs added a file at the same path as ours' rename target
                    if oe.oid != te_at_new.oid || oe.mode != te_at_new.mode {
                        let path_str = String::from_utf8_lossy(ours_new_path).to_string();
                        // This is a rename/add conflict
                        has_conflicts = true;
                        stage_entry(&mut index, oe, 2);
                        stage_entry(&mut index, te_at_new, 3);
                        conflict_descriptions.push(("rename/add".to_string(), path_str));
                    }
                }
            }
        }

        // Handle "add-source": if theirs has a different file at base_path
        // (the original was renamed away, and a new file was added), include it.
        // And since ours' version at base_path was used for the merge at the
        // rename target, ours doesn't also get a file at base_path.
        if let Some(te_at_base) = theirs.get(base_path) {
            if be.is_none_or(|b| te_at_base.oid != b.oid) {
                // Theirs has a new/different file at the old path (add-source)
                if let Some(oe_at_base) = ours.get(base_path) {
                    // Ours also has something at this path — but ours' version
                    // represents their modification of the original (used in
                    // rename merge above). So this is an add (theirs' new file)
                    // vs nothing from ours at this path.
                    // But wait — if ours' file at base_path is different from the
                    // base AND different from what we'd expect from just modifying
                    // the original, it might be a genuine ours addition too.
                    // For now, include theirs' add-source.
                    let _ = oe_at_base; // ours' changes already merged into rename target
                    index.entries.push(te_at_base.clone());
                } else {
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
                        if let Some(te) = theirs.get(theirs_new_path) {
                            let path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                            has_conflicts = true;
                            if let Some(be) = base.get(base_path) {
                                let mut be_at_new = be.clone();
                                be_at_new.path = theirs_new_path.clone();
                                stage_entry(&mut index, &be_at_new, 1);
                            }
                            stage_entry(&mut index, te, 3);
                            conflict_descriptions.push(("rename/rename".to_string(), path_str));
                            // Also add the theirs version to the working tree
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
        let te = theirs.get(theirs_new_path); // The renamed file in theirs
        let oe = ours.get(base_path); // Ours' version at original path

        if let (Some(be), Some(te)) = (be, te) {
            if let Some(oe) = oe {
                // Ours also has the file at the old path — merge content at theirs' new path
                if be.oid == oe.oid && be.mode == oe.mode {
                    // Ours didn't modify — just use theirs (renamed version)
                    index.entries.push(te.clone());
                } else if oe.oid == te.oid {
                    // Both made same change
                    let mut entry = te.clone();
                    entry.path = theirs_new_path.clone();
                    index.entries.push(entry);
                } else {
                    // Both modified — try content merge at new path
                    let path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                    match try_content_merge(repo, be, oe, te, ours_label, their_name, favor, diff_algorithm)? {
                        ContentMergeResult::Clean(merged_oid, mode) => {
                            let mut entry = te.clone();
                            entry.oid = merged_oid;
                            entry.mode = mode;
                            index.entries.push(entry);
                        }
                        ContentMergeResult::Conflict(content) => {
                            has_conflicts = true;
                            let mut be_at_new = be.clone();
                            be_at_new.path = theirs_new_path.clone();
                            stage_entry(&mut index, &be_at_new, 1);
                            let mut oe_at_new = oe.clone();
                            oe_at_new.path = theirs_new_path.clone();
                            stage_entry(&mut index, &oe_at_new, 2);
                            stage_entry(&mut index, te, 3);
                            conflict_descriptions.push(("content".to_string(), path_str.clone()));
                            conflict_files.push((path_str, content));
                        }
                    }
                }
            } else {
                // Ours deleted the original — theirs renamed it → rename/delete conflict
                has_conflicts = true;
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
                conflict_descriptions.push(("rename/delete".to_string(), format!(
                    "{base_path_str} deleted in {ours_label} and renamed to {new_path_str} in {their_name}. Version {their_name} of {new_path_str} left in tree."
                )));
            }

            // If ours also has a NEW file at theirs_new_path (add/add at rename target)
            if let Some(oe_at_new) = ours.get(theirs_new_path) {
                if !base.contains_key(theirs_new_path)
                    && (te.oid != oe_at_new.oid || te.mode != oe_at_new.mode)
                {
                    let path_str = String::from_utf8_lossy(theirs_new_path).to_string();
                    has_conflicts = true;
                    stage_entry(&mut index, oe_at_new, 2);
                    stage_entry(&mut index, te, 3);
                    conflict_descriptions.push(("rename/add".to_string(), path_str));
                }
            }

            // Handle "add-source": theirs renamed base_path away, but theirs may also
            // have a NEW file at base_path (add-source pattern: rename + add at source).
            // Also handle ours' file at base_path: ours' modification of the original
            // was used for the merge at the rename target, so we should not also keep
            // it at base_path. But theirs' add-source at base_path should be included.
            if let Some(te_at_base) = theirs.get(base_path) {
                if te_at_base.oid != be.oid {
                    // Theirs has a genuinely new file at the old path (add-source)
                    index.entries.push(te_at_base.clone());
                }
            }
        }
    }

    // Second pass: handle non-rename paths
    for path in &all_paths {
        if handled_paths.contains(path) {
            continue;
        }

        let b = base.get(path);
        let o = ours.get(path);
        let t = theirs.get(path);

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
            // Added only by ours
            (None, Some(oe), None) => {
                index.entries.push(oe.clone());
            }
            // Added only by theirs
            (None, None, Some(te)) => {
                index.entries.push(te.clone());
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
                match try_content_merge(repo, be, oe, te, ours_label, their_name, favor, diff_algorithm)? {
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
                        conflict_descriptions.push(("content".to_string(), path_str.clone()));
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
                            stage_entry(&mut index, be, 1);
                            stage_entry(&mut index, te, 3);
                            conflict_descriptions.push(("modify/delete".to_string(), path_str));
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
                            stage_entry(&mut index, be, 1);
                            stage_entry(&mut index, oe, 2);
                            conflict_descriptions.push(("modify/delete".to_string(), path_str));
                        }
                    }
                }
            }
            // Both added different content — try content merge with empty base
            (None, Some(oe), Some(te)) => {
                let path_str = String::from_utf8_lossy(path).to_string();
                match try_content_merge_add_add(repo, oe, te, ours_label, their_name, favor, diff_algorithm)? {
                    ContentMergeResult::Clean(merged_oid, mode) => {
                        let mut entry = oe.clone();
                        entry.oid = merged_oid;
                        entry.mode = mode;
                        index.entries.push(entry);
                    }
                    ContentMergeResult::Conflict(content) => {
                        has_conflicts = true;
                        stage_entry(&mut index, oe, 2);
                        stage_entry(&mut index, te, 3);
                        conflict_descriptions.push(("add/add".to_string(), path_str.clone()));
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

enum ContentMergeResult {
    /// Clean merge: (blob oid, mode).
    Clean(ObjectId, u32),
    /// Conflict: merged content with markers.
    Conflict(Vec<u8>),
}

/// Try a content-level three-way merge for a single file.
fn try_content_merge(
    repo: &Repository,
    base: &IndexEntry,
    ours: &IndexEntry,
    theirs: &IndexEntry,
    ours_label: &str,
    theirs_label: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
) -> Result<ContentMergeResult> {
    let base_obj = repo.odb.read(&base.oid)?;
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    // Check .gitattributes for binary marking
    let path_str = String::from_utf8_lossy(&ours.path).to_string();
    let is_attr_binary = {
        let wt_path = repo
            .work_tree
            .as_deref()
            .unwrap_or(std::path::Path::new("."));
        let attrs = grit_lib::crlf::load_gitattributes(wt_path);
        if let Ok(config) = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true) {
            let file_attrs = grit_lib::crlf::get_file_attrs(&attrs, &path_str, &config);
            file_attrs.text == grit_lib::crlf::TextAttr::Unset
        } else {
            false
        }
    };

    // If any is binary (by content or attribute), conflict (unless -X ours/theirs resolves it)
    if is_attr_binary
        || merge_file::is_binary(&base_obj.data)
        || merge_file::is_binary(&ours_obj.data)
        || merge_file::is_binary(&theirs_obj.data)
    {
        match favor {
            MergeFavor::Ours => {
                let oid = repo.odb.write(ObjectKind::Blob, &ours_obj.data)?;
                return Ok(ContentMergeResult::Clean(oid, ours.mode));
            }
            MergeFavor::Theirs => {
                let oid = repo.odb.write(ObjectKind::Blob, &theirs_obj.data)?;
                return Ok(ContentMergeResult::Clean(oid, theirs.mode));
            }
            MergeFavor::None | MergeFavor::Union => {
                // Binary conflict — keep ours in working tree
                return Ok(ContentMergeResult::Conflict(ours_obj.data.clone()));
            }
        }
    }

    let input = MergeInput {
        base: &base_obj.data,
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: ours_label,
        label_base: "base",
        label_theirs: theirs_label,
        favor,
        style: ConflictStyle::Merge,
        marker_size: 7,
        diff_algorithm: diff_algorithm.map(|s| s.to_string()),
    };

    let output = merge_file::merge(&input)?;
    let mode = ours.mode; // Use ours mode by default

    if output.conflicts == 0 {
        let oid = repo.odb.write(ObjectKind::Blob, &output.content)?;
        Ok(ContentMergeResult::Clean(oid, mode))
    } else {
        Ok(ContentMergeResult::Conflict(output.content))
    }
}

/// Try content merge for add/add conflicts (empty base).
fn try_content_merge_add_add(
    repo: &Repository,
    ours: &IndexEntry,
    theirs: &IndexEntry,
    ours_label: &str,
    theirs_label: &str,
    favor: MergeFavor,
    diff_algorithm: Option<&str>,
) -> Result<ContentMergeResult> {
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    if merge_file::is_binary(&ours_obj.data) || merge_file::is_binary(&theirs_obj.data) {
        return Ok(ContentMergeResult::Conflict(ours_obj.data.clone()));
    }

    let input = MergeInput {
        base: &[], // empty base for add/add
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: ours_label,
        label_base: "base",
        label_theirs: theirs_label,
        favor,
        style: ConflictStyle::Merge,
        marker_size: 7,
        diff_algorithm: diff_algorithm.map(|s| s.to_string()),
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

fn stage_entry(index: &mut Index, src: &IndexEntry, stage: u8) {
    let mut e = src.clone();
    e.flags = (e.flags & 0x0FFF) | ((stage as u16) << 12);
    index.entries.push(e);
}

/// Get the tree OID from a commit.
fn commit_tree(repo: &Repository, commit_oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&commit_oid)?;
    let commit = parse_commit(&obj.data)?;
    Ok(commit.tree)
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
    resolve_revision(repo, spec).map_err(|e| anyhow::anyhow!("{}: {}", spec, e))
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

fn build_merge_message(
    head: &HeadState,
    branch_name: &str,
    custom: Option<&str>,
    repo: &Repository,
) -> String {
    if let Some(msg) = custom {
        return ensure_trailing_newline(msg);
    }
    // Determine if the merge target is a tag, branch, or commit
    let kind = if resolve_ref(&repo.git_dir, &format!("refs/tags/{branch_name}")).is_ok() {
        "tag"
    } else if resolve_ref(&repo.git_dir, &format!("refs/remotes/{branch_name}")).is_ok() {
        "remote-tracking branch"
    } else {
        "branch"
    };
    let base_msg = format!("Merge {kind} '{branch_name}'");
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
    for path in old_entries.keys() {
        if !new_paths.contains(path.as_slice()) {
            let path_str = String::from_utf8_lossy(path);
            let abs = work_tree.join(path_str.as_ref());
            if abs.exists() {
                let _ = fs::remove_file(&abs);
            }
        }
    }
    Ok(())
}

/// Checkout index entries to working tree.
fn checkout_entries(repo: &Repository, work_tree: &Path, index: &Index) -> Result<()> {
    // Load gitattributes and config for CRLF conversion
    let attr_rules = grit_lib::crlf::load_gitattributes(work_tree);
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let conv = config
        .as_ref()
        .map(grit_lib::crlf::ConversionConfig::from_config);

    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
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
                grit_lib::crlf::convert_to_worktree(&obj.data, &path_str, conv, &file_attrs, None)
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
            let mut result: Vec<String> = lines
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

fn passthrough_current_merge_invocation() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let Some(idx) = argv.iter().position(|arg| arg == "merge") else {
        bail!("failed to determine merge arguments");
    };
    let passthrough_args = argv
        .get(idx + 1..)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    git_passthrough::run("merge", &passthrough_args)
}
