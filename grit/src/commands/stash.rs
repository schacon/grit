//! `grit stash` — stash the changes in a dirty working directory away.
//!
//! Saves uncommitted changes (staged and/or unstaged) as special merge commits
//! on `refs/stash` with a reflog for history.
//!
//! Stash commits have 2 or 3 parents:
//!   1. HEAD at the time of stashing
//!   2. A commit recording the index state
//!   3. (optional) A commit recording untracked files
//!
//! Subcommands: push, save, list, show, pop, apply, drop, clear, branch, create, store.

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::diff::{diff_index_to_tree, diff_index_to_worktree};
use grit_lib::error::Error;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::objects::{
    parse_commit, parse_tree, serialize_commit, serialize_tree, CommitData, ObjectId, ObjectKind,
    TreeEntry,
};
use grit_lib::odb::Odb;
use grit_lib::reflog::{read_reflog, reflog_path};
use grit_lib::refs::{resolve_ref, write_ref};
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;
use time::OffsetDateTime;

/// Arguments for `grit stash`.
#[derive(Debug, ClapArgs)]
#[command(about = "Stash the changes in a dirty working directory away")]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<StashCommand>,

    /// Message for the stash entry (shorthand for `push -m`).
    #[arg(short = 'm', long = "message", global = true)]
    pub message: Option<String>,

    /// Keep staged changes in the index.
    #[arg(short = 'k', long = "keep-index", global = true)]
    pub keep_index: bool,

    /// Also stash untracked files.
    #[arg(short = 'u', long = "include-untracked", global = true)]
    pub include_untracked: bool,

    /// Quiet mode — suppress output messages.
    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,

    /// Pathspec arguments (for bare `grit stash -- <path>`).
    #[arg(last = true)]
    pub pathspec: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum StashCommand {
    /// Save changes and clean the working tree.
    Push {
        /// Message for the stash entry.
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
        /// Keep staged changes in the index.
        #[arg(short = 'k', long = "keep-index")]
        keep_index: bool,
        /// Revert keep-index (default behavior).
        #[arg(long = "no-keep-index")]
        no_keep_index: bool,
        /// Also stash untracked files.
        #[arg(short = 'u', long = "include-untracked")]
        include_untracked: bool,
        /// Only stash staged changes.
        #[arg(short = 'S', long = "staged")]
        staged: bool,
        /// Interactive patch mode (select hunks to stash).
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// Quiet mode.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Pathspec arguments.
        #[arg(trailing_var_arg = true)]
        pathspec: Vec<String>,
    },
    /// Save changes (legacy; same as push).
    Save {
        /// Message for the stash entry.
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
        /// Keep staged changes in the index.
        #[arg(short = 'k', long = "keep-index")]
        keep_index: bool,
        /// Also stash untracked files.
        #[arg(short = 'u', long = "include-untracked")]
        include_untracked: bool,
        /// Quiet mode.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Legacy positional message.
        #[arg(trailing_var_arg = true)]
        legacy_message: Vec<String>,
    },
    /// List stash entries.
    List {
        /// Extra arguments passed to git log.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show the diff of a stash entry.
    Show {
        /// Show as patch (diff).
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// Show stat (default).
        #[arg(long = "stat")]
        stat: bool,
        /// Patience diff algorithm.
        #[arg(long = "patience")]
        patience: bool,
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Apply stash and remove it.
    Pop {
        /// Also restore the index state.
        #[arg(long = "index")]
        index: bool,
        /// Quiet mode.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Apply stash without removing it.
    Apply {
        /// Also restore the index state.
        #[arg(long = "index")]
        index: bool,
        /// Quiet mode.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Remove a stash entry.
    Drop {
        /// Quiet mode.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Remove all stash entries.
    Clear,
    /// Create a branch from a stash entry.
    Branch {
        /// Branch name to create.
        branch_name: String,
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Create a stash commit without updating refs/stash.
    Create {
        /// Message for the stash entry.
        #[arg(trailing_var_arg = true)]
        message: Vec<String>,
    },
    /// Store a given stash commit in the stash reflog.
    Store {
        /// Message for the reflog entry.
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
        /// Quiet mode.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
        /// The commit to store.
        commit: String,
    },
}

/// Run `grit stash`.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        None => {
            // Bare `grit stash` == `grit stash push`
            // But if there are pathspec args, treat as `stash push -- <pathspec>`
            do_push(PushOpts {
                message: args.message,
                keep_index: args.keep_index,
                no_keep_index: false,
                include_untracked: args.include_untracked,
                staged: false,
                quiet: args.quiet,
                pathspec: args.pathspec,
            })
        }
        Some(StashCommand::Push {
            message,
            keep_index,
            no_keep_index,
            include_untracked,
            staged,
            patch,
            quiet,
            pathspec,
        }) => {
            if patch {
                bail!("interactive patch mode (stash push -p) is not yet implemented");
            }
            let msg = message.or(args.message);
            let ki = keep_index || args.keep_index;
            let iu = include_untracked || args.include_untracked;
            let q = quiet || args.quiet;
            do_push(PushOpts {
                message: msg,
                keep_index: ki,
                no_keep_index,
                include_untracked: iu,
                staged,
                quiet: q,
                pathspec,
            })
        }
        Some(StashCommand::Save {
            message,
            keep_index,
            include_untracked,
            quiet,
            legacy_message,
        }) => {
            // `stash save` uses positional args as message if no -m
            let msg = message.or(args.message).or_else(|| {
                if legacy_message.is_empty() {
                    None
                } else {
                    Some(legacy_message.join(" "))
                }
            });
            let ki = keep_index || args.keep_index;
            let iu = include_untracked || args.include_untracked;
            let q = quiet || args.quiet;
            do_push(PushOpts {
                message: msg,
                keep_index: ki,
                no_keep_index: false,
                include_untracked: iu,
                staged: false,
                quiet: q,
                pathspec: Vec::new(),
            })
        }
        Some(StashCommand::List { args: list_args }) => do_list(list_args),
        Some(StashCommand::Show { patch, stat: _, patience: _, args: show_args }) => {
            // Parse stash ref from trailing args (non-flag args)
            let stash_ref = show_args.iter().find(|a| !a.starts_with('-')).cloned();
            do_show(stash_ref, patch)
        }
        Some(StashCommand::Pop { index, quiet, stash }) => {
            let q = quiet || args.quiet;
            do_pop(stash, index, q)
        }
        Some(StashCommand::Apply { index, quiet, stash }) => {
            let q = quiet || args.quiet;
            do_apply(stash, false, index, q)
        }
        Some(StashCommand::Drop { quiet, stash }) => {
            let q = quiet || args.quiet;
            do_drop(stash, q)
        }
        Some(StashCommand::Clear) => do_clear(),
        Some(StashCommand::Branch { branch_name, stash }) => do_branch(branch_name, stash),
        Some(StashCommand::Create { message }) => {
            let msg = if message.is_empty() {
                None
            } else {
                Some(message.join(" "))
            };
            do_create(msg)
        }
        Some(StashCommand::Store { message, quiet, commit }) => {
            let q = quiet || args.quiet;
            do_store(commit, message, q)
        }
    }
}

// ---------------------------------------------------------------------------
// Push options
// ---------------------------------------------------------------------------

struct PushOpts {
    message: Option<String>,
    keep_index: bool,
    no_keep_index: bool,
    include_untracked: bool,
    staged: bool,
    quiet: bool,
    pathspec: Vec<String>,
}

// ---------------------------------------------------------------------------
// Push (save)
// ---------------------------------------------------------------------------

fn do_push(opts: PushOpts) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot stash in a bare repository"))?
        .to_path_buf();

    let head = resolve_head(&repo.git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("cannot stash on an unborn branch"))?;

    // Load index
    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Get the HEAD commit's tree for comparison
    let head_obj = repo.odb.read(head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;

    // Check if there are staged changes (index vs HEAD tree)
    let staged = diff_index_to_tree(&repo.odb, &index, Some(&head_commit.tree))?;
    // Check if there are unstaged changes (worktree vs index)
    let unstaged = diff_index_to_worktree(&repo.odb, &index, &work_tree)?;

    // Filter by pathspec if given
    let has_pathspec = !opts.pathspec.is_empty();

    // Find untracked files if requested (and no pathspec)
    let untracked_files = if opts.include_untracked && !has_pathspec && !opts.staged {
        find_untracked_files(&work_tree, &index)?
    } else {
        Vec::new()
    };

    if has_pathspec {
        // Pathspec mode: only stash files matching the pathspec
        return do_push_pathspec(&repo, &work_tree, &head, head_oid, &index, &opts);
    }

    if opts.staged {
        // --staged: only stash staged changes, leave worktree alone
        return do_push_staged(&repo, &work_tree, &head, head_oid, &index, &opts);
    }

    if staged.is_empty() && unstaged.is_empty() && untracked_files.is_empty() {
        if !opts.quiet {
            eprintln!("No local changes to save");
        }
        return Ok(());
    }

    let stash_oid = create_stash_commit(
        &repo,
        &head,
        head_oid,
        &index,
        &work_tree,
        opts.message.as_deref(),
        opts.include_untracked,
        &untracked_files,
    )?;

    // Update refs/stash
    update_stash_ref(&repo, &stash_oid, &stash_reflog_msg(&head, opts.message.as_deref()))?;

    // Determine effective keep_index
    let effective_keep_index = if opts.no_keep_index {
        false
    } else {
        opts.keep_index
    };

    // Clean working tree: reset to HEAD state
    if effective_keep_index {
        // Reset working tree to index state (keep staged changes in both index and worktree)
        reset_worktree_to_index(&repo, &index, &work_tree)?;
    } else {
        // Reset index and working tree to HEAD
        reset_to_head(&repo, head_oid, &work_tree)?;
    }

    // Remove untracked files if they were stashed
    if opts.include_untracked {
        for f in &untracked_files {
            let path = work_tree.join(f);
            let _ = fs::remove_file(&path);
            if let Some(parent) = path.parent() {
                remove_empty_dirs(parent, &work_tree);
            }
        }
    }

    if !opts.quiet {
        let msg = stash_save_msg(&head, opts.message.as_deref());
        eprintln!("Saved working directory and index state {msg}");
    }

    Ok(())
}

/// Push with pathspec: only stash specific files.
fn do_push_pathspec(
    repo: &Repository,
    work_tree: &Path,
    head: &HeadState,
    head_oid: &ObjectId,
    index: &Index,
    opts: &PushOpts,
) -> Result<()> {
    let head_obj = repo.odb.read(head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;

    // Get all changes
    let staged = diff_index_to_tree(&repo.odb, index, Some(&head_commit.tree))?;
    let unstaged = diff_index_to_worktree(&repo.odb, index, work_tree)?;

    // Filter by pathspec
    let matching_staged: Vec<_> = staged
        .iter()
        .filter(|e| matches_pathspec(e.path(), &opts.pathspec))
        .collect();
    let matching_unstaged: Vec<_> = unstaged
        .iter()
        .filter(|e| matches_pathspec(e.path(), &opts.pathspec))
        .collect();

    if matching_staged.is_empty() && matching_unstaged.is_empty() {
        if !opts.quiet {
            eprintln!("No local changes to save");
        }
        return Ok(());
    }

    let now = OffsetDateTime::now_utc();
    let identity = resolve_identity(repo, now)?;

    // 1. Create index-state commit (current full index)
    let index_tree_oid = write_tree_from_index(&repo.odb, index, "")?;
    let index_commit_data = CommitData {
        tree: index_tree_oid,
        parents: vec![*head_oid],
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: format!("index on {}", branch_description(head)),
        raw_message: None,
    };
    let index_commit_bytes = serialize_commit(&index_commit_data);
    let index_commit_oid = repo.odb.write(ObjectKind::Commit, &index_commit_bytes)?;

    // 2. Create working-tree state commit
    let wt_tree_oid = create_worktree_tree(&repo.odb, index, work_tree)?;

    let stash_msg = stash_save_msg(head, opts.message.as_deref());
    let reflog_msg = stash_reflog_msg(head, opts.message.as_deref());

    let stash_commit = CommitData {
        tree: wt_tree_oid,
        parents: vec![*head_oid, index_commit_oid],
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: stash_msg.clone(),
    raw_message: None,
    };
    let stash_bytes = serialize_commit(&stash_commit);
    let stash_oid = repo.odb.write(ObjectKind::Commit, &stash_bytes)?;

    // Update refs/stash
    update_stash_ref(repo, &stash_oid, &reflog_msg)?;

    // Now restore only the matched files to HEAD state, leave the rest alone
    let head_tree_entries = flatten_tree_full(&repo.odb, &head_commit.tree, "")?;
    let head_map: std::collections::BTreeMap<String, &FlatTreeEntry> = head_tree_entries
        .iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    // Collect paths that match pathspec
    let mut matched_paths: BTreeSet<String> = BTreeSet::new();
    for e in &matching_staged {
        if let Some(p) = e.new_path.as_ref().or(e.old_path.as_ref()) {
            matched_paths.insert(p.clone());
        }
    }
    for e in &matching_unstaged {
        if let Some(p) = e.new_path.as_ref().or(e.old_path.as_ref()) {
            matched_paths.insert(p.clone());
        }
    }

    // Rebuild index: for matched paths, reset to HEAD state; for others, keep current
    let mut new_index = index.clone();
    for path in &matched_paths {
        let path_bytes = path.as_bytes();
        if let Some(head_entry) = head_map.get(path.as_str()) {
            // Restore file to HEAD state
            let file_path = work_tree.join(path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let blob = repo.odb.read(&head_entry.oid)?;
            if head_entry.mode == MODE_SYMLINK {
                let target = String::from_utf8(blob.data)
                    .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
                if file_path.exists() || file_path.symlink_metadata().is_ok() {
                    let _ = fs::remove_file(&file_path);
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &file_path)?;
            } else {
                fs::write(&file_path, &blob.data)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if head_entry.mode == MODE_EXECUTABLE {
                        let perms = std::fs::Permissions::from_mode(0o755);
                        fs::set_permissions(&file_path, perms)?;
                    }
                }
            }
            // Update index entry
            if let Some(ie) = new_index.get_mut(path_bytes, 0) {
                ie.oid = head_entry.oid;
                ie.mode = head_entry.mode;
            }
        } else {
            // File was added (not in HEAD) — remove from worktree and index
            let file_path = work_tree.join(path);
            let _ = fs::remove_file(&file_path);
            if let Some(parent) = file_path.parent() {
                remove_empty_dirs(parent, work_tree);
            }
            new_index.remove(path_bytes);
        }
    }

    new_index.write(&repo.index_path())?;

    if !opts.quiet {
        eprintln!("Saved working directory and index state {stash_msg}");
    }

    Ok(())
}

/// Push with --staged: only stash staged changes.
fn do_push_staged(
    repo: &Repository,
    work_tree: &Path,
    head: &HeadState,
    head_oid: &ObjectId,
    index: &Index,
    opts: &PushOpts,
) -> Result<()> {
    let head_obj = repo.odb.read(head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;

    let staged = diff_index_to_tree(&repo.odb, index, Some(&head_commit.tree))?;
    if staged.is_empty() {
        if !opts.quiet {
            eprintln!("No local changes to save");
        }
        return Ok(());
    }

    let now = OffsetDateTime::now_utc();
    let identity = resolve_identity(repo, now)?;

    // The "index commit" is the current index state (which has staged changes)
    let index_tree_oid = write_tree_from_index(&repo.odb, index, "")?;
    let index_commit_data = CommitData {
        tree: index_tree_oid,
        parents: vec![*head_oid],
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: format!("index on {}", branch_description(head)),
        raw_message: None,
    };
    let index_commit_bytes = serialize_commit(&index_commit_data);
    let index_commit_oid = repo.odb.write(ObjectKind::Commit, &index_commit_bytes)?;

    // The stash commit tree is the index tree (since we're only stashing staged)
    let stash_msg = stash_save_msg(head, opts.message.as_deref());
    let reflog_msg = stash_reflog_msg(head, opts.message.as_deref());

    let stash_commit = CommitData {
        tree: index_tree_oid,
        parents: vec![*head_oid, index_commit_oid],
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: stash_msg.clone(),
    raw_message: None,
    };
    let stash_bytes = serialize_commit(&stash_commit);
    let stash_oid = repo.odb.write(ObjectKind::Commit, &stash_bytes)?;

    // Update refs/stash
    update_stash_ref(repo, &stash_oid, &reflog_msg)?;

    // Reset index back to HEAD (unstage the changes)
    // For files that were newly added (not in HEAD), also remove from worktree
    // For files that were modified, restore them to HEAD content in worktree
    let head_tree_entries = flatten_tree_full(&repo.odb, &head_commit.tree, "")?;
    let head_paths: std::collections::BTreeSet<String> = head_tree_entries.iter().map(|e| e.path.clone()).collect();

    // Revert staged changes in the worktree
    for change in &staged {
        if let Some(path) = change.new_path.as_ref().or(change.old_path.as_ref()) {
            let file_path = work_tree.join(path);
            if !head_paths.contains(path) {
                // New file (added) — remove from worktree
                let _ = fs::remove_file(&file_path);
                if let Some(parent) = file_path.parent() {
                    remove_empty_dirs(parent, work_tree);
                }
            } else {
                // Modified file — restore HEAD content
                for te in &head_tree_entries {
                    if te.path == *path {
                        let blob = repo.odb.read(&te.oid)?;
                        if te.mode == MODE_SYMLINK {
                            let target = String::from_utf8(blob.data)
                                .map_err(|_| anyhow::anyhow!("symlink not UTF-8"))?;
                            if file_path.exists() || file_path.symlink_metadata().is_ok() {
                                let _ = fs::remove_file(&file_path);
                            }
                            #[cfg(unix)]
                            std::os::unix::fs::symlink(&target, &file_path)?;
                        } else {
                            fs::write(&file_path, &blob.data)?;
                        }
                        break;
                    }
                }
            }
        }
    }

    let new_index = build_index_from_tree(&repo.odb, &head_tree_entries)?;
    new_index.write(&repo.index_path())?;

    if !opts.quiet {
        eprintln!("Saved working directory and index state {stash_msg}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Create (make stash commit without updating ref)
// ---------------------------------------------------------------------------

fn do_create(message: Option<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot stash in a bare repository"))?
        .to_path_buf();

    let head = resolve_head(&repo.git_dir)?;
    let head_oid = head
        .oid()
        .ok_or_else(|| anyhow::anyhow!("cannot stash on an unborn branch"))?;

    let index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    let head_obj = repo.odb.read(head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;
    let staged = diff_index_to_tree(&repo.odb, &index, Some(&head_commit.tree))?;
    let unstaged = diff_index_to_worktree(&repo.odb, &index, &work_tree)?;

    if staged.is_empty() && unstaged.is_empty() {
        // No changes — exit silently (git stash create does this)
        return Ok(());
    }

    let stash_oid = create_stash_commit(
        &repo,
        &head,
        head_oid,
        &index,
        &work_tree,
        message.as_deref(),
        false,
        &[],
    )?;

    println!("{}", stash_oid.to_hex());
    Ok(())
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

fn do_store(commit_hex: String, message: Option<String>, quiet: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    let oid = ObjectId::from_hex(&commit_hex)
        .context("not a valid object")?;

    // Verify it's a commit
    let obj = repo.odb.read(&oid)?;
    if obj.kind != ObjectKind::Commit {
        bail!("not a stash-like commit: {commit_hex}");
    }

    let msg = message.unwrap_or_else(|| {
        // Try to use the commit message
        if let Ok(cd) = parse_commit(&obj.data) {
            format!("On {}", cd.message.lines().next().unwrap_or("(no message)"))
        } else {
            "Created via \"git stash store\".".to_string()
        }
    });

    update_stash_ref(&repo, &oid, &msg)?;

    if !quiet {
        // git store is normally quiet
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

fn do_list(extra_args: Vec<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let entries = read_reflog(&repo.git_dir, "refs/stash")?;

    // Check if -p flag was passed
    let show_patch = extra_args.iter().any(|a| a == "-p" || a == "--patch");
    let show_cc = extra_args.iter().any(|a| a == "--cc");

    // Entries are in file order (oldest first), display newest first
    for (i, entry) in entries.iter().rev().enumerate() {
        println!("stash@{{{i}}}: {}", entry.message);
        if show_patch || show_cc {
            // Show diff for this stash entry
            if let Ok(stash_oid) = resolve_stash_ref(&repo, Some(&format!("stash@{{{i}}}"))) {
                let _ = show_stash_diff(&repo, &stash_oid, true);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Show
// ---------------------------------------------------------------------------

fn do_show(stash_ref: Option<String>, patch: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stash_oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    if patch {
        show_stash_diff(&repo, &stash_oid, true)?;
    } else {
        // Default: --stat format
        show_stash_stat(&repo, &stash_oid)?;
    }

    Ok(())
}

fn show_stash_diff(repo: &Repository, stash_oid: &ObjectId, _with_hunks: bool) -> Result<()> {
    let obj = repo.odb.read(stash_oid)?;
    let stash_commit = parse_commit(&obj.data)?;

    let parent_oid = stash_commit
        .parents
        .first()
        .ok_or_else(|| anyhow::anyhow!("corrupt stash commit: no parents"))?;
    let parent_obj = repo.odb.read(parent_oid)?;
    let parent_commit = parse_commit(&parent_obj.data)?;

    let old_entries = flatten_tree_full(&repo.odb, &parent_commit.tree, "")?;
    let new_entries = flatten_tree_full(&repo.odb, &stash_commit.tree, "")?;

    show_tree_diff(&repo.odb, &old_entries, &new_entries)?;

    Ok(())
}

fn show_stash_stat(repo: &Repository, stash_oid: &ObjectId) -> Result<()> {
    let obj = repo.odb.read(stash_oid)?;
    let stash_commit = parse_commit(&obj.data)?;

    let parent_oid = stash_commit
        .parents
        .first()
        .ok_or_else(|| anyhow::anyhow!("corrupt stash commit: no parents"))?;
    let parent_obj = repo.odb.read(parent_oid)?;
    let parent_commit = parse_commit(&parent_obj.data)?;

    let old_entries = flatten_tree_full(&repo.odb, &parent_commit.tree, "")?;
    let new_entries = flatten_tree_full(&repo.odb, &stash_commit.tree, "")?;

    // Build maps
    use std::collections::BTreeMap;
    let mut old_map: BTreeMap<&str, &FlatTreeEntry> = BTreeMap::new();
    for e in &old_entries {
        old_map.insert(&e.path, e);
    }
    let mut new_map: BTreeMap<&str, &FlatTreeEntry> = BTreeMap::new();
    for e in &new_entries {
        new_map.insert(&e.path, e);
    }

    let mut all_paths: BTreeSet<&str> = BTreeSet::new();
    for e in &old_entries {
        all_paths.insert(&e.path);
    }
    for e in &new_entries {
        all_paths.insert(&e.path);
    }

    struct StatEntry {
        path: String,
        insertions: usize,
        deletions: usize,
    }

    let mut stats: Vec<StatEntry> = Vec::new();
    let mut total_insertions = 0usize;
    let mut total_deletions = 0usize;
    let mut total_files = 0usize;

    for path in &all_paths {
        match (old_map.get(path), new_map.get(path)) {
            (Some(o), Some(n)) if o.oid != n.oid || o.mode != n.mode => {
                let (ins, del) = count_line_changes(&repo.odb, &o.oid, &n.oid)?;
                total_insertions += ins;
                total_deletions += del;
                total_files += 1;
                stats.push(StatEntry {
                    path: path.to_string(),
                    insertions: ins,
                    deletions: del,
                });
            }
            (None, Some(n)) => {
                let blob = repo.odb.read(&n.oid)?;
                let ins = String::from_utf8_lossy(&blob.data).lines().count();
                total_insertions += ins;
                total_files += 1;
                stats.push(StatEntry {
                    path: path.to_string(),
                    insertions: ins,
                    deletions: 0,
                });
            }
            (Some(o), None) => {
                let blob = repo.odb.read(&o.oid)?;
                let del = String::from_utf8_lossy(&blob.data).lines().count();
                total_deletions += del;
                total_files += 1;
                stats.push(StatEntry {
                    path: path.to_string(),
                    insertions: 0,
                    deletions: del,
                });
            }
            _ => {}
        }
    }

    if stats.is_empty() {
        return Ok(());
    }

    // Find max path width and max changes for scaling
    let max_path_len = stats.iter().map(|s| s.path.len()).max().unwrap_or(0);
    let max_changes = stats
        .iter()
        .map(|s| s.insertions + s.deletions)
        .max()
        .unwrap_or(0);

    // Scale bar to fit in terminal (max bar width ~50)
    let max_bar_width = 50usize;
    let scale = if max_changes > max_bar_width {
        max_bar_width as f64 / max_changes as f64
    } else {
        1.0
    };

    for s in &stats {
        let changes = s.insertions + s.deletions;
        let bar_ins = (s.insertions as f64 * scale).ceil() as usize;
        let bar_del = (s.deletions as f64 * scale).ceil() as usize;
        let bar = format!(
            "{}{}",
            "+".repeat(bar_ins),
            "-".repeat(bar_del)
        );
        println!(
            " {:<width$} | {:>3} {}",
            s.path,
            changes,
            bar,
            width = max_path_len,
        );
    }

    // Summary line
    let mut summary_parts = Vec::new();
    summary_parts.push(format!(
        " {} file{} changed",
        total_files,
        if total_files == 1 { "" } else { "s" }
    ));
    if total_insertions > 0 {
        summary_parts.push(format!(
            " {} insertion{}(+)",
            total_insertions,
            if total_insertions == 1 { "" } else { "s" }
        ));
    }
    if total_deletions > 0 {
        summary_parts.push(format!(
            " {} deletion{}(-)",
            total_deletions,
            if total_deletions == 1 { "" } else { "s" }
        ));
    }
    println!("{}", summary_parts.join(","));

    Ok(())
}

fn count_line_changes(odb: &Odb, old_oid: &ObjectId, new_oid: &ObjectId) -> Result<(usize, usize)> {
    let old_blob = odb.read(old_oid)?;
    let new_blob = odb.read(new_oid)?;
    let old_text = String::from_utf8_lossy(&old_blob.data);
    let new_text = String::from_utf8_lossy(&new_blob.data);

    use similar::TextDiff;
    let diff = TextDiff::from_lines(&old_text as &str, &new_text as &str);
    let mut ins = 0usize;
    let mut del = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => ins += 1,
            similar::ChangeTag::Delete => del += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    Ok((ins, del))
}

// ---------------------------------------------------------------------------
// Apply / Pop
// ---------------------------------------------------------------------------

fn do_pop(stash_ref: Option<String>, index: bool, quiet: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot apply stash in a bare repository"))?
        .to_path_buf();

    let stash_index = parse_stash_index(stash_ref.as_deref())?;
    let stash_oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    // Apply the stash
    let had_conflicts = apply_stash_impl(&repo, &work_tree, &stash_oid, index, quiet)?;

    if had_conflicts {
        // On conflict, do NOT drop the stash entry
        if !quiet {
            eprintln!("The stash entry is kept in case you need it again.");
        }
        // Return error to indicate failure
        bail!("Conflicts in index. Try without --index or use stash branch.");
    }

    // Drop if no conflicts
    drop_stash_entry(&repo, stash_index)?;
    if !quiet {
        let dropped_oid = stash_oid.to_hex();
        eprintln!(
            "Dropped refs/stash@{{{stash_index}}} ({short})",
            short = &dropped_oid[..7]
        );
    }

    Ok(())
}

fn do_apply(stash_ref: Option<String>, _drop_after: bool, index: bool, quiet: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot apply stash in a bare repository"))?
        .to_path_buf();

    let stash_oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    apply_stash_impl(&repo, &work_tree, &stash_oid, index, quiet)?;

    Ok(())
}

/// Apply a stash. Returns true if there were conflicts.
fn apply_stash_impl(
    repo: &Repository,
    work_tree: &Path,
    stash_oid: &ObjectId,
    restore_index: bool,
    _quiet: bool,
) -> Result<bool> {
    let obj = repo.odb.read(stash_oid)?;
    let stash_commit = parse_commit(&obj.data)?;

    if stash_commit.parents.len() < 2 {
        bail!("corrupt stash commit: expected at least 2 parents");
    }

    let head_at_stash = &stash_commit.parents[0];
    let index_commit_oid = &stash_commit.parents[1];

    // Load current index
    let current_index = match Index::load(&repo.index_path()) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Read stash trees
    let stash_tree_entries = flatten_tree_full(&repo.odb, &stash_commit.tree, "")?;

    // Read HEAD-at-stash tree (base)
    let head_at_stash_obj = repo.odb.read(head_at_stash)?;
    let head_at_stash_commit = parse_commit(&head_at_stash_obj.data)?;
    let base_tree_entries = flatten_tree_full(&repo.odb, &head_at_stash_commit.tree, "")?;

    use std::collections::BTreeMap;
    let base_map: BTreeMap<String, &FlatTreeEntry> = base_tree_entries
        .iter()
        .map(|e| (e.path.clone(), e))
        .collect();
    let stash_map: BTreeMap<String, &FlatTreeEntry> = stash_tree_entries
        .iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    // Find files changed in the stash working tree vs base
    let mut wt_changes: BTreeMap<String, Option<&FlatTreeEntry>> = BTreeMap::new();
    for (path, stash_entry) in &stash_map {
        match base_map.get(path) {
            Some(base_entry) if base_entry.oid != stash_entry.oid || base_entry.mode != stash_entry.mode => {
                wt_changes.insert(path.clone(), Some(stash_entry));
            }
            None => {
                wt_changes.insert(path.clone(), Some(stash_entry));
            }
            _ => {}
        }
    }
    // Track deletions (in base but not in stash)
    for (path, _) in &base_map {
        if !stash_map.contains_key(path) {
            wt_changes.insert(path.clone(), None); // None = deleted
        }
    }

    // Check for conflicts: does the worktree have local modifications to files
    // that the stash also wants to change?
    for (path, _) in &wt_changes {
        let file_path = work_tree.join(path);
        // Get the current index entry for this file
        if let Some(idx_entry) = current_index.get(path.as_bytes(), 0) {
            // Read the worktree file
            match fs::read(&file_path) {
                Ok(contents) => {
                    // Check if worktree differs from index
                    if let Ok(idx_blob) = repo.odb.read(&idx_entry.oid) {
                        if contents != idx_blob.data {
                            // Worktree has local changes that would be overwritten
                            bail!("error: Your local changes to the following files would be overwritten by merge:\n\t{path}\nPlease commit your changes or stash them before you merge.");
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // File doesn't exist in worktree — could be deleted locally
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    // Read index commit tree
    let idx_obj = repo.odb.read(index_commit_oid)?;
    let idx_commit = parse_commit(&idx_obj.data)?;
    let idx_tree_entries = flatten_tree_full(&repo.odb, &idx_commit.tree, "")?;
    let idx_map: BTreeMap<String, &FlatTreeEntry> = idx_tree_entries
        .iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    // Apply working tree changes
    for (path, change) in &wt_changes {
        let file_path = work_tree.join(path);
        match change {
            Some(entry) => {
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let blob = repo.odb.read(&entry.oid)?;
                if entry.mode == MODE_SYMLINK {
                    let target = String::from_utf8(blob.data)
                        .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
                    if file_path.exists() || file_path.symlink_metadata().is_ok() {
                        let _ = fs::remove_file(&file_path);
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&target, &file_path)?;
                } else {
                    fs::write(&file_path, &blob.data)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if entry.mode == MODE_EXECUTABLE {
                            let perms = std::fs::Permissions::from_mode(0o755);
                            fs::set_permissions(&file_path, perms)?;
                        }
                    }
                }
            }
            None => {
                // Deleted in stash
                let _ = fs::remove_file(&file_path);
                if let Some(parent) = file_path.parent() {
                    remove_empty_dirs(parent, work_tree);
                }
            }
        }
    }

    // Update the index
    let mut new_index = current_index.clone();

    if restore_index {
        // --index: restore the index to the stash's index state for changed files
        for (path, idx_entry) in &idx_map {
            let base_oid = base_map.get(path).map(|e| &e.oid);
            if base_oid != Some(&idx_entry.oid) {
                // This file was staged differently from base in the stash
                let path_bytes = path.as_bytes();
                if let Some(ie) = new_index.get_mut(path_bytes, 0) {
                    ie.oid = idx_entry.oid;
                    ie.mode = idx_entry.mode;
                } else {
                    let flags = if path.len() > 0xFFF {
                        0xFFF
                    } else {
                        path.len() as u16
                    };
                    new_index.entries.push(IndexEntry {
                        ctime_sec: 0,
                        ctime_nsec: 0,
                        mtime_sec: 0,
                        mtime_nsec: 0,
                        dev: 0,
                        ino: 0,
                        mode: idx_entry.mode,
                        uid: 0,
                        gid: 0,
                        size: 0,
                        oid: idx_entry.oid,
                        flags,
                        flags_extended: None,
                        path: path_bytes.to_vec(),
                    });
                }
            }
        }
        // Handle files added in the index but not in base
        // (already covered above)
        new_index.sort();
    }
    // else: no --index, keep the current index as-is

    new_index.write(&repo.index_path())?;

    // Apply untracked files if present (3rd parent)
    if stash_commit.parents.len() >= 3 {
        let ut_oid = &stash_commit.parents[2];
        let ut_obj = repo.odb.read(ut_oid)?;
        let ut_commit = parse_commit(&ut_obj.data)?;
        let ut_entries = flatten_tree_full(&repo.odb, &ut_commit.tree, "")?;
        for entry in &ut_entries {
            let file_path = work_tree.join(&entry.path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let blob = repo.odb.read(&entry.oid)?;
            fs::write(&file_path, &blob.data)?;
        }
    }

    Ok(false)
}

// ---------------------------------------------------------------------------
// Branch
// ---------------------------------------------------------------------------

fn do_branch(branch_name: String, stash_ref: Option<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot use stash branch in a bare repository"))?
        .to_path_buf();

    let stash_index = parse_stash_index(stash_ref.as_deref())?;
    let stash_oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    // Read the stash commit to get the parent (HEAD at stash time)
    let obj = repo.odb.read(&stash_oid)?;
    let stash_commit = parse_commit(&obj.data)?;

    if stash_commit.parents.len() < 2 {
        bail!("corrupt stash commit: expected at least 2 parents");
    }

    let head_at_stash = &stash_commit.parents[0];

    // Check if the branch already exists
    let branch_ref = format!("refs/heads/{branch_name}");
    if resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        bail!("a]branch named '{branch_name}' already exists");
    }

    // Create the branch at the stash's parent commit
    write_ref(&repo.git_dir, &branch_ref, head_at_stash)?;

    // Switch HEAD to the new branch
    let head_path = repo.git_dir.join("HEAD");
    fs::write(&head_path, format!("ref: {branch_ref}\n"))?;

    // Reset working tree and index to head_at_stash
    reset_to_head(&repo, head_at_stash, &work_tree)?;

    // Now apply the stash with --index
    apply_stash_impl(&repo, &work_tree, &stash_oid, true, false)?;

    // Drop the stash entry
    drop_stash_entry(&repo, stash_index)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Drop
// ---------------------------------------------------------------------------

fn do_drop(stash_ref: Option<String>, quiet: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stash_index = parse_stash_index(stash_ref.as_deref())?;

    // Verify it exists
    let oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    drop_stash_entry(&repo, stash_index)?;
    if !quiet {
        let hex = oid.to_hex();
        eprintln!(
            "Dropped refs/stash@{{{stash_index}}} ({short})",
            short = &hex[..7]
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Clear
// ---------------------------------------------------------------------------

fn do_clear() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stash_path = repo.git_dir.join("refs").join("stash");
    let log_path = reflog_path(&repo.git_dir, "refs/stash");
    let _ = fs::remove_file(&stash_path);
    let _ = fs::remove_file(&log_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a path matches any of the given pathspecs.
fn matches_pathspec(path: &str, pathspecs: &[String]) -> bool {
    for spec in pathspecs {
        // Strip leading "--" if present (from clap parsing)
        let spec = spec.strip_prefix("--").unwrap_or(spec);
        // Simple matching: exact match or glob-like prefix
        if path == spec {
            return true;
        }
        // Simple glob: if spec ends with *, match prefix
        if let Some(prefix) = spec.strip_suffix('*') {
            if path.starts_with(prefix) {
                return true;
            }
        }
        // Directory prefix: spec/ matches all files under spec
        if spec.ends_with('/') && path.starts_with(spec) {
            return true;
        }
        // If spec doesn't contain '/' and doesn't have glob, also try as directory prefix
        if !spec.contains('/') && path.starts_with(&format!("{spec}/")) {
            return true;
        }
        // Glob pattern matching for patterns like "foo b*"
        if spec.contains('*') {
            if glob_match(spec, path) {
                return true;
            }
        }
    }
    false
}

/// Simple glob matching (only supports * wildcard).
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = text[pos..].find(part) {
            if i == 0 && found != 0 {
                return false;
            }
            pos += found + part.len();
        } else {
            return false;
        }
    }
    // If pattern doesn't end with *, text must end exactly
    if !pattern.ends_with('*') {
        return pos == text.len();
    }
    true
}

/// Create a stash commit and return its OID (does NOT update refs/stash).
fn create_stash_commit(
    repo: &Repository,
    head: &HeadState,
    head_oid: &ObjectId,
    index: &Index,
    work_tree: &Path,
    message: Option<&str>,
    include_untracked: bool,
    untracked_files: &[String],
) -> Result<ObjectId> {
    let now = OffsetDateTime::now_utc();
    let identity = resolve_identity(repo, now)?;

    // 1. Create index-state commit (tree from current index)
    let index_tree_oid = write_tree_from_index(&repo.odb, index, "")?;
    let index_commit_data = CommitData {
        tree: index_tree_oid,
        parents: vec![*head_oid],
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: format!("index on {}", branch_description(head)),
        raw_message: None,
    };
    let index_commit_bytes = serialize_commit(&index_commit_data);
    let index_commit_oid = repo.odb.write(ObjectKind::Commit, &index_commit_bytes)?;

    // 2. Optionally create untracked-files commit
    let untracked_commit_oid = if include_untracked && !untracked_files.is_empty() {
        let tree_oid = create_untracked_tree(&repo.odb, work_tree, untracked_files)?;
        let ut_commit = CommitData {
            tree: tree_oid,
            parents: vec![*head_oid],
            author: identity.clone(),
            committer: identity.clone(),
            encoding: None,
            message: format!("untracked files on {}", branch_description(head)),
            raw_message: None,
        };
        let ut_bytes = serialize_commit(&ut_commit);
        Some(repo.odb.write(ObjectKind::Commit, &ut_bytes)?)
    } else {
        None
    };

    // 3. Create the working-tree state commit
    let wt_tree_oid = create_worktree_tree(&repo.odb, index, work_tree)?;

    let stash_msg = stash_save_msg(head, message);

    let mut parents = vec![*head_oid, index_commit_oid];
    if let Some(ut_oid) = untracked_commit_oid {
        parents.push(ut_oid);
    }

    let stash_commit = CommitData {
        tree: wt_tree_oid,
        parents,
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: stash_msg,
    raw_message: None,
    };
    let stash_bytes = serialize_commit(&stash_commit);
    let stash_oid = repo.odb.write(ObjectKind::Commit, &stash_bytes)?;

    Ok(stash_oid)
}

/// Generate the stash save message (used as commit message).
fn stash_save_msg(head: &HeadState, message: Option<&str>) -> String {
    match message {
        Some(msg) => format!("On {}: {msg}", branch_short_name(head)),
        None => format!("WIP on {}", branch_description(head)),
    }
}

/// Generate the stash reflog message.
fn stash_reflog_msg(head: &HeadState, message: Option<&str>) -> String {
    stash_save_msg(head, message)
}

/// Update refs/stash and its reflog.
fn update_stash_ref(repo: &Repository, stash_oid: &ObjectId, message: &str) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let identity = resolve_identity(repo, now)?;

    let old_stash = resolve_ref(&repo.git_dir, "refs/stash").ok();
    let zero_oid = ObjectId::from_hex("0000000000000000000000000000000000000000")?;
    let old_oid = old_stash.unwrap_or(zero_oid);

    write_ref(&repo.git_dir, "refs/stash", stash_oid)?;
    grit_lib::refs::append_reflog(
        &repo.git_dir,
        "refs/stash",
        &old_oid,
        stash_oid,
        &identity,
        message,
    )?;

    Ok(())
}

/// Parse stash@{N} notation and return the index N.
fn parse_stash_index(stash_ref: Option<&str>) -> Result<usize> {
    match stash_ref {
        None => Ok(0),
        Some(s) => {
            // Accept "stash@{N}" or just "N"
            if let Some(rest) = s.strip_prefix("stash@{") {
                if let Some(num) = rest.strip_suffix('}') {
                    return num
                        .parse::<usize>()
                        .context("invalid stash index");
                }
            }
            // Try as plain number
            if let Ok(n) = s.parse::<usize>() {
                return Ok(n);
            }
            bail!("invalid stash reference: {s}");
        }
    }
}

/// Resolve a stash reference to an ObjectId.
fn resolve_stash_ref(repo: &Repository, stash_ref: Option<&str>) -> Result<ObjectId> {
    let index = parse_stash_index(stash_ref)?;
    let entries = read_reflog(&repo.git_dir, "refs/stash")?;
    if entries.is_empty() {
        bail!("No stash entries");
    }
    // Entries are oldest-first in the file, newest-first for stash@{0}
    let rev_index = entries.len().checked_sub(1 + index);
    match rev_index {
        Some(i) => Ok(entries[i].new_oid),
        None => bail!("stash@{{{index}}} does not exist"),
    }
}

/// Drop a stash entry by index.
fn drop_stash_entry(repo: &Repository, index: usize) -> Result<()> {
    let entries = read_reflog(&repo.git_dir, "refs/stash")?;
    if entries.is_empty() {
        bail!("No stash entries");
    }
    if index >= entries.len() {
        bail!("stash@{{{index}}} does not exist");
    }

    // Remove the entry from the reflog
    grit_lib::reflog::delete_reflog_entries(&repo.git_dir, "refs/stash", &[index])?;

    // Update refs/stash to point to the new top entry (or remove it)
    let remaining = read_reflog(&repo.git_dir, "refs/stash")?;
    if remaining.is_empty() {
        let _ = fs::remove_file(repo.git_dir.join("refs").join("stash"));
    } else {
        let top = &remaining
            .last()
            .ok_or_else(|| anyhow::anyhow!("stash entries unexpectedly empty"))?
            .new_oid;
        write_ref(&repo.git_dir, "refs/stash", top)?;
    }

    Ok(())
}

/// Get a branch description string for stash messages (e.g. "main: abc1234 commit msg").
fn branch_description(head: &HeadState) -> String {
    match head {
        HeadState::Branch { refname, oid, .. } => {
            let name = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            match oid {
                Some(oid) => format!("{name}: {}", &oid.to_hex()[..7]),
                None => name.to_string(),
            }
        }
        HeadState::Detached { oid } => format!("(no branch): {}", &oid.to_hex()[..7]),
        HeadState::Invalid => "(invalid HEAD)".to_string(),
    }
}

/// Get just the branch short name.
fn branch_short_name(head: &HeadState) -> String {
    match head {
        HeadState::Branch { refname, .. } => {
            refname
                .strip_prefix("refs/heads/")
                .unwrap_or(refname)
                .to_string()
        }
        HeadState::Detached { oid } => format!("(no branch): {}", &oid.to_hex()[..7]),
        HeadState::Invalid => "(invalid HEAD)".to_string(),
    }
}

/// Resolve committer identity from config/env.
fn resolve_identity(repo: &Repository, now: OffsetDateTime) -> Result<String> {
    let config = ConfigSet::load(Some(&repo.git_dir), true)?;
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_NAME").ok())
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_EMAIL").ok())
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    Ok(format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}"))
}

/// A flat tree entry for diffing.
struct FlatTreeEntry {
    path: String,
    mode: u32,
    oid: ObjectId,
}

/// Recursively flatten a tree into (path, mode, oid) entries.
fn flatten_tree_full(odb: &Odb, tree_oid: &ObjectId, prefix: &str) -> Result<Vec<FlatTreeEntry>> {
    let obj = odb.read(tree_oid)?;
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();
    for entry in entries {
        let entry_name = String::from_utf8_lossy(&entry.name).to_string();
        let full_path = if prefix.is_empty() {
            entry_name
        } else {
            format!("{prefix}/{entry_name}")
        };
        if entry.mode == 0o40000 {
            let sub = flatten_tree_full(odb, &entry.oid, &full_path)?;
            result.extend(sub);
        } else {
            result.push(FlatTreeEntry {
                path: full_path,
                mode: entry.mode,
                oid: entry.oid,
            });
        }
    }
    Ok(result)
}

/// Show diff between two flattened trees.
fn show_tree_diff(
    odb: &Odb,
    old: &[FlatTreeEntry],
    new: &[FlatTreeEntry],
) -> Result<()> {
    use std::collections::BTreeMap;

    let mut old_map: BTreeMap<&str, &FlatTreeEntry> = BTreeMap::new();
    for e in old {
        old_map.insert(&e.path, e);
    }
    let mut new_map: BTreeMap<&str, &FlatTreeEntry> = BTreeMap::new();
    for e in new {
        new_map.insert(&e.path, e);
    }

    let mut all_paths: BTreeSet<&str> = BTreeSet::new();
    for e in old {
        all_paths.insert(&e.path);
    }
    for e in new {
        all_paths.insert(&e.path);
    }

    for path in &all_paths {
        match (old_map.get(path), new_map.get(path)) {
            (Some(o), Some(n)) => {
                if o.oid != n.oid || o.mode != n.mode {
                    println!("diff --git a/{path} b/{path}");
                    if o.mode != n.mode {
                        println!("old mode {}", format_mode(o.mode));
                        println!("new mode {}", format_mode(n.mode));
                    }
                    println!(
                        "index {}..{}",
                        &o.oid.to_hex()[..7],
                        &n.oid.to_hex()[..7]
                    );
                    println!("--- a/{path}");
                    println!("+++ b/{path}");
                    show_blob_diff(odb, &o.oid, &n.oid)?;
                }
            }
            (None, Some(n)) => {
                println!("diff --git a/{path} b/{path}");
                println!("new file mode {}", format_mode(n.mode));
                println!("--- /dev/null");
                println!("+++ b/{path}");
                let blob = odb.read(&n.oid)?;
                let text = String::from_utf8_lossy(&blob.data);
                for line in text.lines() {
                    println!("+{line}");
                }
            }
            (Some(o), None) => {
                println!("diff --git a/{path} b/{path}");
                println!("deleted file mode {}", format_mode(o.mode));
                println!("--- a/{path}");
                println!("+++ /dev/null");
                let blob = odb.read(&o.oid)?;
                let text = String::from_utf8_lossy(&blob.data);
                for line in text.lines() {
                    println!("-{line}");
                }
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(())
}

/// Show a simple line diff between two blobs.
fn show_blob_diff(odb: &Odb, old_oid: &ObjectId, new_oid: &ObjectId) -> Result<()> {
    let old_blob = odb.read(old_oid)?;
    let new_blob = odb.read(new_oid)?;
    let old_text = String::from_utf8_lossy(&old_blob.data);
    let new_text = String::from_utf8_lossy(&new_blob.data);

    use similar::TextDiff;
    let diff = TextDiff::from_lines(&old_text as &str, &new_text as &str);
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
            similar::ChangeTag::Equal => " ",
        };
        print!("{sign}{change}");
    }

    Ok(())
}

fn format_mode(mode: u32) -> String {
    format!("{mode:06o}")
}

/// Find untracked files in the working tree.
fn find_untracked_files(work_tree: &Path, index: &Index) -> Result<Vec<String>> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    let mut untracked = Vec::new();
    walk_for_untracked(work_tree, work_tree, &tracked, &mut untracked)?;
    untracked.sort();
    Ok(untracked)
}

fn walk_for_untracked(
    dir: &Path,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    out: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name == ".git" {
            continue;
        }

        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name);

        if path.is_dir() {
            walk_for_untracked(&path, work_tree, tracked, out)?;
        } else if !tracked.contains(&rel) {
            out.push(rel);
        }
    }

    Ok(())
}

/// Create a tree object containing untracked files.
fn create_untracked_tree(
    odb: &Odb,
    work_tree: &Path,
    files: &[String],
) -> Result<ObjectId> {
    use std::collections::BTreeMap;

    struct TreeBuilder {
        blobs: BTreeMap<String, (u32, ObjectId)>,
        subtrees: BTreeMap<String, TreeBuilder>,
    }

    impl TreeBuilder {
        fn new() -> Self {
            Self {
                blobs: BTreeMap::new(),
                subtrees: BTreeMap::new(),
            }
        }

        fn insert(&mut self, path: &str, mode: u32, oid: ObjectId) {
            if let Some(pos) = path.find('/') {
                let dir = &path[..pos];
                let rest = &path[pos + 1..];
                self.subtrees
                    .entry(dir.to_string())
                    .or_insert_with(TreeBuilder::new)
                    .insert(rest, mode, oid);
            } else {
                self.blobs.insert(path.to_string(), (mode, oid));
            }
        }

        fn write(self, odb: &Odb) -> Result<ObjectId> {
            let mut entries = Vec::new();
            for (name, (mode, oid)) in self.blobs {
                entries.push(TreeEntry {
                    mode,
                    name: name.into_bytes(),
                    oid,
                });
            }
            for (name, builder) in self.subtrees {
                let oid = builder.write(odb)?;
                entries.push(TreeEntry {
                    mode: 0o40000,
                    name: name.into_bytes(),
                    oid,
                });
            }
            entries.sort_by(|a, b| {
                let a_name = String::from_utf8_lossy(&a.name);
                let b_name = String::from_utf8_lossy(&b.name);
                let a_key = if a.mode == 0o40000 {
                    format!("{a_name}/")
                } else {
                    a_name.to_string()
                };
                let b_key = if b.mode == 0o40000 {
                    format!("{b_name}/")
                } else {
                    b_name.to_string()
                };
                a_key.cmp(&b_key)
            });
            let data = serialize_tree(&entries);
            Ok(odb.write(ObjectKind::Tree, &data)?)
        }
    }

    let mut builder = TreeBuilder::new();
    for file in files {
        let file_path = work_tree.join(file);
        let data = fs::read(&file_path)?;
        let oid = odb.write(ObjectKind::Blob, &data)?;
        let meta = fs::symlink_metadata(&file_path)?;
        let mode = mode_from_metadata(&meta);
        builder.insert(file, mode, oid);
    }
    builder.write(odb)
}

/// Create a tree representing the working tree state of all tracked files.
fn create_worktree_tree(
    odb: &Odb,
    index: &Index,
    work_tree: &Path,
) -> Result<ObjectId> {
    let mut temp_index = index.clone();

    for entry in &mut temp_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).to_string();
        let file_path = work_tree.join(&path_str);
        match fs::symlink_metadata(&file_path) {
            Ok(meta) => {
                if meta.is_symlink() {
                    let target = fs::read_link(&file_path)?;
                    let target_bytes = target.to_string_lossy().into_owned().into_bytes();
                    let oid = odb.write(ObjectKind::Blob, &target_bytes)?;
                    entry.oid = oid;
                    entry.mode = MODE_SYMLINK;
                } else {
                    let data = fs::read(&file_path)?;
                    let oid = odb.write(ObjectKind::Blob, &data)?;
                    entry.oid = oid;
                    entry.mode = mode_from_metadata(&meta);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
                || e.raw_os_error() == Some(20) /* ENOTDIR */ => {
                entry.oid = ObjectId::from_hex("0000000000000000000000000000000000000000")?;
            }
            Err(e) => return Err(e.into()),
        }
    }

    let zero = ObjectId::from_hex("0000000000000000000000000000000000000000")?;
    temp_index.entries.retain(|e| e.oid != zero);

    write_tree_from_index(odb, &temp_index, "").map_err(Into::into)
}

/// Build an Index from a flattened tree.
fn build_index_from_tree(_odb: &Odb, entries: &[FlatTreeEntry]) -> Result<Index> {
    let mut index = Index::new();
    for entry in entries {
        let path_len = entry.path.len();
        let flags = if path_len > 0xFFF {
            0xFFF
        } else {
            path_len as u16
        };
        index.entries.push(IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode: entry.mode,
            uid: 0,
            gid: 0,
            size: 0,
            oid: entry.oid,
            flags,
            flags_extended: None,
            path: entry.path.as_bytes().to_vec(),
        });
    }
    index.sort();
    Ok(index)
}

/// Reset working tree files to match the index (for --keep-index).
fn reset_worktree_to_index(repo: &Repository, index: &Index, work_tree: &Path) -> Result<()> {
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path);
        let file_path = work_tree.join(path_str.as_ref());
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let blob = repo.odb.read(&entry.oid)?;
        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(blob.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if file_path.exists() || file_path.symlink_metadata().is_ok() {
                let _ = fs::remove_file(&file_path);
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &file_path)?;
        } else {
            fs::write(&file_path, &blob.data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if entry.mode == MODE_EXECUTABLE {
                    let perms = std::fs::Permissions::from_mode(0o755);
                    fs::set_permissions(&file_path, perms)?;
                }
            }
        }
    }
    Ok(())
}

/// Reset index and working tree to HEAD.
fn reset_to_head(repo: &Repository, head_oid: &ObjectId, work_tree: &Path) -> Result<()> {
    let head_obj = repo.odb.read(head_oid)?;
    let head_commit = parse_commit(&head_obj.data)?;

    let tree_entries = flatten_tree_full(&repo.odb, &head_commit.tree, "")?;
    let new_index = build_index_from_tree(&repo.odb, &tree_entries)?;

    // First pass: remove worktree files that are not in HEAD tree
    // (handles type changes like file→directory)
    let head_paths: BTreeSet<String> = tree_entries.iter().map(|e| e.path.clone()).collect();
    remove_worktree_extras(work_tree, work_tree, &head_paths)?;

    for entry in &tree_entries {
        let file_path = work_tree.join(&entry.path);
        if let Some(parent) = file_path.parent() {
            ensure_directory(parent, work_tree)?;
        }
        let blob = repo.odb.read(&entry.oid)?;
        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(blob.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if file_path.exists() || file_path.symlink_metadata().is_ok() {
                let _ = fs::remove_file(&file_path);
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &file_path)?;
        } else {
            fs::write(&file_path, &blob.data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if entry.mode == MODE_EXECUTABLE {
                    let perms = std::fs::Permissions::from_mode(0o755);
                    fs::set_permissions(&file_path, perms)?;
                }
            }
        }
    }

    new_index.write(&repo.index_path())?;
    Ok(())
}

/// Derive file mode from metadata.
fn mode_from_metadata(meta: &std::fs::Metadata) -> u32 {
    if meta.is_symlink() {
        MODE_SYMLINK
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.mode() & 0o111 != 0 {
                MODE_EXECUTABLE
            } else {
                0o100644
            }
        }
        #[cfg(not(unix))]
        {
            0o100644
        }
    }
}

/// Ensure a path is a directory, removing any conflicting file in the way.
fn ensure_directory(dir: &Path, work_tree: &Path) -> Result<()> {
    // Walk from work_tree down to dir, checking each component
    if dir.is_dir() {
        return Ok(());
    }
    // Some ancestor might be a file — remove it
    let rel = dir.strip_prefix(work_tree).unwrap_or(dir);
    let mut current = work_tree.to_path_buf();
    for component in rel.components() {
        current.push(component);
        if current.exists() && !current.is_dir() {
            // A file is blocking where we need a directory
            fs::remove_file(&current)?;
        }
    }
    fs::create_dir_all(dir)?;
    Ok(())
}

/// Remove worktree files/dirs that are not in the target tree set.
/// This handles type changes (e.g., a file `dir` that should become directory `dir/`).
fn remove_worktree_extras(dir: &Path, work_tree: &Path, target_paths: &BTreeSet<String>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name);

        if path.is_dir() {
            // Check if any target path starts with this dir prefix
            let prefix = format!("{rel}/");
            if target_paths.iter().any(|t| t.starts_with(&prefix)) {
                // Recurse — directory is needed
                remove_worktree_extras(&path, work_tree, target_paths)?;
            }
            // Don't remove untracked dirs here — only clean up conflicts
        } else {
            // Check if this file path conflicts with a needed directory
            let prefix = format!("{rel}/");
            if target_paths.iter().any(|t| t.starts_with(&prefix)) {
                // File exists where a directory is needed — remove it
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

/// Remove empty parent directories up to (but not including) the work tree root.
fn remove_empty_dirs(dir: &Path, stop_at: &Path) {
    let mut current = dir.to_path_buf();
    while current != stop_at {
        if fs::read_dir(&current)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            let _ = fs::remove_dir(&current);
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                break;
            }
        } else {
            break;
        }
    }
}
