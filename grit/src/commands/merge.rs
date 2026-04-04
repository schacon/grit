//! `grit merge` — join two or more development histories together.
//!
//! Implements fast-forward, three-way merge with conflict handling,
//! `--squash`, `--no-ff`, `--ff-only`, `--abort`, and `--continue`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use grit_lib::config::ConfigSet;
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
}

/// Run the `merge` command.
pub fn run(args: Args) -> Result<()> {
    if args.abort {
        return merge_abort();
    }
    if args.continue_merge {
        return merge_continue(args.message);
    }

    // Handle -s help early (before commit check)
    if args.strategy.as_deref() == Some("help") {
        eprintln!("Could not find merge strategy 'help'.");
        eprintln!("Available strategies are: octopus ours recursive resolve subtree.");
        std::process::exit(1);
    }

    if args.commits.is_empty() {
        bail!("nothing to merge — please specify a branch or commit");
    }
    if args.ff_only && args.no_ff {
        bail!("cannot combine --ff-only and --no-ff");
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
            "ours" | "subtree" => {
                // "ours" strategy: keep our tree, just make a merge commit.
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
    for xopt in &args.strategy_option {
        match xopt.as_str() {
            "ours" => favor = MergeFavor::Ours,
            "theirs" => favor = MergeFavor::Theirs,
            other => bail!("unknown strategy option: -X {other}"),
        }
    }

    let repo = Repository::discover(None).context("not a git repository")?;
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
        return do_octopus_merge(&repo, &head, head_oid, &args, favor);
    }

    // Resolve merge target
    let merge_oid = resolve_merge_target(&repo, &args.commits[0])?;

    // Handle -s ours: keep our tree, just create merge commit
    if args.strategy.as_deref() == Some("ours") {
        return do_strategy_ours(&repo, &head, head_oid, merge_oid, &args);
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
        if args.no_ff {
            // Force a merge commit even though we could fast-forward
            return do_real_merge(&repo, &head, head_oid, merge_oid, &args, favor);
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

    do_real_merge(&repo, &head, head_oid, merge_oid, &args, favor)
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
        eprintln!(
            "Updating to {}",
            &merge_oid.to_hex()[..7]
        );
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
        eprintln!(
            "Updating {}..{}",
            &head_oid.to_hex()[..7],
            &merge_oid.to_hex()[..7]
        );
        eprintln!("Fast-forward");
    }
    Ok(())
}

/// Perform a real three-way merge.
fn do_real_merge(
    repo: &Repository,
    head: &HeadState,
    head_oid: ObjectId,
    merge_oid: ObjectId,
    args: &Args,
    favor: MergeFavor,
) -> Result<()> {
    // Find merge base
    let bases = grit_lib::merge_base::merge_bases_first_vs_rest(repo, head_oid, &[merge_oid])?;
    if bases.is_empty() {
        bail!("refusing to merge unrelated histories");
    }
    let base_oid = bases[0];

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
    let merge_result =
        merge_trees(repo, &base_entries, &ours_entries, &theirs_entries, head, &args.commits[0], favor)?;

    // Write index
    merge_result.index.write(&repo.index_path())?;

    // Update working tree
    if let Some(ref wt) = repo.work_tree {
        // Remove files that were in ours but are no longer in the merged index
        remove_deleted_files(wt, &ours_entries, &merge_result.index)?;
        checkout_entries(repo, wt, &merge_result.index)?;
        // Write conflict files to working tree
        for (path, content) in &merge_result.conflict_files {
            let abs = wt.join(path);
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, content)?;
        }
    }

    if merge_result.has_conflicts {
        // Write MERGE_HEAD and MERGE_MSG for conflict resolution
        fs::write(
            repo.git_dir.join("MERGE_HEAD"),
            format!("{}\n", merge_oid.to_hex()),
        )?;
        let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), &repo);
        fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
        fs::write(repo.git_dir.join("MERGE_MODE"), "")?;

        // Print per-file conflict messages
        for (ctype, cpath) in &merge_result.conflict_descriptions {
            eprintln!("CONFLICT ({ctype}): Merge conflict in {cpath}");
        }
        eprintln!("Automatic merge failed; fix conflicts and then commit the result.");
        // Return error to signal failure (exit code 1)
        bail!("Automatic merge failed; fix conflicts and then commit the result.");
    }

    if args.squash {
        return do_squash_from_merge(repo, &merge_result.index, head, &args.commits[0], args);
    }

    if args.no_commit {
        // --no-commit: stage the result but don't create the merge commit.
        // Write MERGE_HEAD and MERGE_MSG so that a subsequent `git commit`
        // creates the merge commit with the right parents.
        fs::write(
            repo.git_dir.join("MERGE_HEAD"),
            format!("{}\n", merge_oid.to_hex()),
        )?;
        let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), &repo);
        fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
        fs::write(repo.git_dir.join("MERGE_MODE"), "no-ff\n")?;

        if !args.quiet {
            eprintln!("Automatic merge went well; stopped before committing as requested");
        }
        return Ok(());
    }

    // Create merge commit
    let tree_oid = write_tree_from_index(&repo.odb, &merge_result.index, "")?;
    let mut msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), &repo);

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
) -> Result<()> {
    // Resolve all merge targets
    let mut merge_oids = Vec::new();
    for name in &args.commits {
        merge_oids.push(resolve_merge_target(repo, name)?);
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
        )?;

        if merge_result.has_conflicts {
            // Write the conflict state to disk
            merge_result.index.write(&repo.index_path())?;
            if let Some(ref wt) = repo.work_tree {
                checkout_entries(repo, wt, &merge_result.index)?;
                for (path, content) in &merge_result.conflict_files {
                    let abs = wt.join(path);
                    if let Some(parent) = abs.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&abs, content)?;
                }
            }
            // Write MERGE_HEAD with all remaining merge OIDs
            let merge_head_content: String = merge_oids
                .iter()
                .map(|oid| format!("{}\n", oid.to_hex()))
                .collect();
            fs::write(repo.git_dir.join("MERGE_HEAD"), &merge_head_content)?;
            let msg = build_octopus_merge_message(head, &args.commits, args.message.as_deref());
            fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
            fs::write(repo.git_dir.join("MERGE_MODE"), "")?;
            for (ctype, cpath) in &merge_result.conflict_descriptions {
                eprintln!("CONFLICT ({ctype}): Merge conflict in {cpath}");
            }
            eprintln!("Automatic merge failed; fix conflicts and then commit the result.");
            bail!("Automatic merge failed; fix conflicts and then commit the result.");
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

    if args.no_commit {
        let merge_head_content: String = merge_oids
            .iter()
            .map(|oid| format!("{}\n", oid.to_hex()))
            .collect();
        fs::write(repo.git_dir.join("MERGE_HEAD"), &merge_head_content)?;
        let msg = build_octopus_merge_message(head, &args.commits, args.message.as_deref());
        fs::write(repo.git_dir.join("MERGE_MSG"), &msg)?;
        fs::write(repo.git_dir.join("MERGE_MODE"), "no-ff\n")?;
        if !args.quiet {
            eprintln!("Automatic merge went well; stopped before committing as requested");
        }
        return Ok(());
    }

    let tree_oid = write_tree_from_index(&repo.odb, &final_index, "")?;
    let msg = build_octopus_merge_message(head, &args.commits, args.message.as_deref());

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

/// Build merge message for octopus merges.
fn build_octopus_merge_message(head: &HeadState, branch_names: &[String], custom: Option<&str>) -> String {
    if let Some(msg) = custom {
        return ensure_trailing_newline(msg);
    }
    // Git uses "Merge branches 'a', 'b' and 'c'" for octopus
    let formatted = if branch_names.len() == 2 {
        format!("Merge branches '{}' and '{}'", branch_names[0], branch_names[1])
    } else {
        let last = branch_names.last().unwrap();
        let rest: Vec<String> = branch_names[..branch_names.len() - 1]
            .iter()
            .map(|n| format!("'{}'", n))
            .collect();
        format!("Merge branches {} and '{}'", rest.join(", "), last)
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
    let msg = build_merge_message(head, &args.commits[0], args.message.as_deref(), &repo);

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
    let msg = format!(
        "Squashed commit of the following:\n\ncommit {}\n",
        merge_oid.to_hex()
    );
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
    branch_name: &str,
    args: &Args,
) -> Result<()> {
    index.write(&repo.index_path())?;

    let msg = format!(
        "Squashed commit of the following:\n\nMerge branch '{}'\n",
        branch_name
    );
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

/// Perform tree-level three-way merge.
fn merge_trees(
    repo: &Repository,
    base: &HashMap<Vec<u8>, IndexEntry>,
    ours: &HashMap<Vec<u8>, IndexEntry>,
    theirs: &HashMap<Vec<u8>, IndexEntry>,
    head: &HeadState,
    their_name: &str,
    favor: MergeFavor,
) -> Result<MergeResult> {
    let mut all_paths = BTreeSet::new();
    all_paths.extend(base.keys().cloned());
    all_paths.extend(ours.keys().cloned());
    all_paths.extend(theirs.keys().cloned());

    let mut index = Index::new();
    let mut has_conflicts = false;
    let mut conflict_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut conflict_descriptions: Vec<(String, String)> = Vec::new();

    let ours_label = head.branch_name().unwrap_or("HEAD");

    for path in &all_paths {
        let b = base.get(path);
        let o = ours.get(path);
        let t = theirs.get(path);

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
                // Skip — removed from both sides
            }
            // All three differ — content-level merge
            (Some(be), Some(oe), Some(te)) => {
                let path_str = String::from_utf8_lossy(path).to_string();
                match try_content_merge(repo, be, oe, te, ours_label, their_name, favor)? {
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
                if be.oid == te.oid && be.mode == te.mode {
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
                if be.oid == oe.oid && be.mode == oe.mode {
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
                match try_content_merge_add_add(repo, oe, te, ours_label, their_name, favor)? {
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
) -> Result<ContentMergeResult> {
    let base_obj = repo.odb.read(&base.oid)?;
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    // If any is binary, conflict
    if merge_file::is_binary(&base_obj.data)
        || merge_file::is_binary(&ours_obj.data)
        || merge_file::is_binary(&theirs_obj.data)
    {
        // Binary conflict — keep ours in working tree
        return Ok(ContentMergeResult::Conflict(ours_obj.data.clone()));
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
) -> Result<ContentMergeResult> {
    let ours_obj = repo.odb.read(&ours.oid)?;
    let theirs_obj = repo.odb.read(&theirs.oid)?;

    if merge_file::is_binary(&ours_obj.data) || merge_file::is_binary(&theirs_obj.data) {
        return Ok(ContentMergeResult::Conflict(ours_obj.data.clone()));
    }

    let input = MergeInput {
        base: &[],  // empty base for add/add
        ours: &ours_obj.data,
        theirs: &theirs_obj.data,
        label_ours: ours_label,
        label_base: "base",
        label_theirs: theirs_label,
        favor,
        style: ConflictStyle::Merge,
        marker_size: 7,
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

fn build_merge_message(head: &HeadState, branch_name: &str, custom: Option<&str>, repo: &Repository) -> String {
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
            fs::write(&abs_path, &obj.data)?;
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

    let timestamp = std::env::var(date_var).unwrap_or_else(|_| {
        let epoch = now.unix_timestamp();
        let offset = now.offset();
        let hours = offset.whole_hours();
        let minutes = offset.minutes_past_hour().unsigned_abs();
        format!("{epoch} {hours:+03}{minutes:02}")
    });

    Ok(format!("{name} <{email}> {timestamp}"))
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}
