//! `grit checkout` — switch branches or restore working tree files.
//!
//! Supports:
//! - `checkout <branch>` — switch to a branch, updating HEAD, index, and working tree.
//! - `checkout -b <new-branch> [<start>]` — create and switch to a new branch.
//! - `checkout <commit>` — detach HEAD at a commit.
//! - `checkout [<tree-ish>] -- <paths>` — restore specific files.
//! - `-f` / `--force` — discard local changes when switching.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::crlf;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs::{self, append_reflog};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{abbreviate_object_id, resolve_revision};
use grit_lib::state::{resolve_head, HeadState};

/// Arguments for `grit checkout`.
#[derive(Debug, ClapArgs)]
#[command(about = "Switch branches or restore working tree files")]
pub struct Args {
    /// Create a new branch and switch to it.
    #[arg(short = 'b')]
    pub new_branch: Option<String>,

    /// Create (or force-reset) a new branch and switch to it.
    #[arg(short = 'B', conflicts_with = "new_branch")]
    pub force_branch: Option<String>,

    /// Create a new orphan branch (no parent commit).
    #[arg(long = "orphan")]
    pub orphan: Option<String>,

    /// Force: discard local changes.
    #[arg(short = 'f', long = "force", hide = true)]
    pub force: bool,

    /// Suppress feedback messages.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Detach HEAD at the named commit (even if it is a branch).
    #[arg(long = "detach", short = 'd', conflicts_with_all = ["new_branch", "force_branch", "orphan"])]
    pub detach: bool,

    /// Set up tracking (upstream) configuration for the new branch.
    #[arg(long = "track", short = 't', action = clap::ArgAction::SetTrue)]
    pub track: bool,

    /// Do not set up tracking configuration.
    #[arg(long = "no-track", hide = true)]
    pub no_track: bool,

    /// Do not keep files that are not in the source tree (path mode).
    #[arg(long = "no-overlay", hide = true)]
    pub no_overlay: bool,

    /// Keep overlay behaviour (default, for explicitness).
    #[arg(long = "overlay")]
    pub overlay: bool,

    /// Interactively select hunks to discard.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Merge local modifications when switching branches.
    #[arg(short = 'm', long = "merge")]
    pub merge: bool,

    /// Check out their version for unmerged files.
    #[arg(long = "ours")]
    pub ours: bool,

    /// Check out our version for unmerged files.
    #[arg(long = "theirs")]
    pub theirs: bool,

    /// Conflict style (merge or diff3).
    #[arg(long = "conflict")]
    pub conflict: Option<String>,

    /// Lines of context for --patch.
    #[arg(long = "unified", short = 'U')]
    pub unified: Option<usize>,

    /// Maximum number of context lines between diff hunks.
    #[arg(long = "inter-hunk-context")]
    pub inter_hunk_context: Option<usize>,

    /// Do not fail on entries with skip-worktree bit set.
    #[arg(long = "ignore-skip-worktree-bits")]
    pub ignore_skip_worktree_bits: bool,

    /// Do not check if another worktree has it checked out.
    #[arg(long = "ignore-other-worktrees")]
    pub ignore_other_worktrees: bool,

    /// Recurse into submodules.
    #[arg(long = "recurse-submodules")]
    pub recurse_submodules: bool,

    /// Auto-advance to next conflict.
    #[arg(long = "auto-advance")]
    pub auto_advance: bool,

    /// Display progress.
    #[arg(long = "progress")]
    pub progress: bool,

    /// Guess branch name from remote tracking branches (default).
    #[arg(long = "guess")]
    pub guess: bool,

    /// Do not guess branch name from remote tracking branches.
    #[arg(long = "no-guess")]
    pub no_guess: bool,

    /// NUL-terminated pathspec from file.
    #[arg(long = "pathspec-file-nul")]
    pub pathspec_file_nul: bool,

    /// Read pathspec from file.
    #[arg(long = "pathspec-from-file")]
    pub pathspec_from_file: Option<String>,

    /// Remaining positional arguments: `[<branch|commit>] [--] [<paths>...]`
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub rest: Vec<String>,
}

/// Run `grit checkout`.
use std::cell::Cell;

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// Print to stderr unless quiet mode is enabled.
macro_rules! checkout_eprintln {
    ($($arg:tt)*) => {
        QUIET.with(|q| {
            if !q.get() {
                eprintln!($($arg)*);
            }
        })
    };
}

pub fn run(args: Args) -> Result<()> {
    QUIET.with(|q| q.set(args.quiet));
    let repo = Repository::discover(None).context("not a git repository")?;

    // Detect if `--` was used in the original command line. Clap strips a
    // leading `--` from trailing_var_arg, so we check the raw args.
    let raw_args: Vec<String> = std::env::args().collect();
    let has_separator = raw_args.iter().any(|a| a == "--");

    // Parse rest into (target, paths) handling `--` separator
    let (target, paths) = split_target_and_paths(&args.rest, has_separator);

    // Resolve @{-N} in start point if present
    let target = target.map(|t| resolve_at_minus(&repo, &t).unwrap_or(t));

    // Case: checkout -p (interactive patch mode)
    if args.patch {
        return checkout_patch(&repo, target.as_deref(), &paths);
    }

    // Case: checkout --orphan <name>
    if let Some(ref orphan_name) = args.orphan {
        return create_orphan_branch(&repo, orphan_name);
    }

    // Case: checkout -B <name> [<start_point>] (force create/reset)
    if let Some(ref force_branch_name) = args.force_branch {
        // -B takes at most one positional arg (start point)
        if !paths.is_empty() || args.rest.len() > 1 {
            bail!("too many arguments for -B");
        }
        let result =
            force_create_and_switch_branch(&repo, force_branch_name, target.as_deref(), args.force);
        if result.is_ok() && !args.no_track {
            maybe_setup_tracking(&repo, force_branch_name, target.as_deref(), args.track)?;
        }
        return result;
    }

    // Case 1: checkout -b <new_branch> [<start_point>]
    if let Some(ref new_branch_name) = args.new_branch {
        // -b takes at most one positional arg (start point)
        if !paths.is_empty() || args.rest.len() > 1 {
            bail!("too many arguments for -b");
        }
        // Capture the current HEAD branch before checkout (for tracking setup)
        let pre_head_branch = if target.is_none() && args.track {
            match resolve_head(&repo.git_dir) {
                Ok(HeadState::Branch { short_name, .. }) => Some(short_name),
                _ => None,
            }
        } else {
            None
        };
        let effective_target = target.as_deref().or(pre_head_branch.as_deref());
        let result =
            create_and_switch_branch(&repo, new_branch_name, target.as_deref(), args.force);
        if result.is_ok() && !args.no_track {
            maybe_setup_tracking(&repo, new_branch_name, effective_target, args.track)?;
        }
        return result;
    }

    // Case 2: checkout [<tree-ish>] -- <paths>  (path restore)
    if !paths.is_empty() {
        return checkout_paths(&repo, target.as_deref(), &paths, args.no_overlay);
    }

    // Case: checkout -f (no args) — force reset working tree to HEAD
    if args.force && target.is_none() && paths.is_empty() {
        return force_reset_to_head(&repo);
    }

    // Case 3: checkout -- (with no paths and no target) is a no-op
    // Case 4: checkout <branch-or-commit>
    let target = match target {
        Some(t) if t.is_empty() => {
            bail!("fatal: empty string is not a valid pathspec or branch name")
        }
        Some(t) => t,
        None => {
            if args.detach {
                // `checkout --detach` with no target: detach at current HEAD
                match resolve_head(&repo.git_dir)? {
                    HeadState::Branch { oid: Some(oid), .. } | HeadState::Detached { oid } => {
                        return detach_head(&repo, &oid, args.force);
                    }
                    _ => bail!("cannot detach HEAD on unborn branch"),
                }
            }
            bail!("you must specify a branch, commit, or paths to checkout")
        }
    };

    // Handle @{-N} syntax: Nth previously checked out branch
    if target.starts_with("@{-") && target.ends_with('}') {
        if let Ok(n) = target[3..target.len() - 1].parse::<usize>() {
            let prev = resolve_nth_previous_branch(&repo, n)?;
            let branch_ref = format!("refs/heads/{prev}");
            if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
                return switch_branch(&repo, &prev, &branch_ref, args.force);
            }
            if let Ok(oid) = resolve_to_commit(&repo, &prev) {
                return detach_head(&repo, &oid, args.force);
            }
            bail!("error: previous branch '{}' not found", prev);
        }
    }

    // Handle "checkout -" — switch to previous branch via reflog
    if target == "-" {
        let prev = resolve_previous_branch(&repo)?;
        let branch_ref = format!("refs/heads/{prev}");
        if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
            return switch_branch(&repo, &prev, &branch_ref, args.force);
        }
        // Not a branch — try as a commit (detached HEAD)
        if let Ok(oid) = resolve_to_commit(&repo, &prev) {
            return detach_head(&repo, &oid, args.force);
        }
        bail!("error: previous branch '{}' not found", prev);
    }

    // Handle "checkout HEAD" — no-op when on a branch (don't detach)
    // But with -f, force-reset the working tree
    if target == "HEAD" && !args.detach {
        if args.force {
            return force_reset_to_head(&repo);
        }
        return Ok(());
    }

    // If --detach, force detached HEAD even for branch names
    if args.detach {
        // --detach takes at most one argument
        if args.rest.len() > 1 {
            bail!("--detach does not take a path argument");
        }
        match resolve_to_commit(&repo, &target) {
            Ok(oid) => return detach_head(&repo, &oid, args.force),
            Err(e) => bail!("cannot detach HEAD at '{}': {}", target, e),
        }
    }

    // Try as a branch first
    let branch_ref = format!("refs/heads/{target}");
    if !args.detach && refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        return switch_branch(&repo, &target, &branch_ref, args.force);
    }

    // Try as a commit (detached HEAD)
    match resolve_to_commit(&repo, &target) {
        Ok(oid) => detach_head(&repo, &oid, args.force),
        Err(_) => {
            // Fallback: try as a pathspec (git checkout <file> without --).
            // If the target looks like a tracked file, restore it from HEAD.
            let paths = vec![target.clone()];
            match checkout_paths(&repo, None, &paths, false) {
                Ok(()) => Ok(()),
                Err(_) => bail!(
                    "pathspec '{}' did not match any file(s) known to git",
                    target
                ),
            }
        }
    }
}

/// Split positional arguments into (target, paths) around `--`.
///
/// `has_separator` indicates whether `--` appeared in the raw CLI args.
/// Clap strips the leading `--` when it is the first trailing arg, so we
/// need this external signal to distinguish `checkout -- file` from
/// `checkout file`.
fn split_target_and_paths(rest: &[String], has_separator: bool) -> (Option<String>, Vec<String>) {
    if rest.is_empty() {
        return (None, vec![]);
    }

    // Look for an explicit `--` still present in the args (happens when
    // there is a target before `--`, e.g. `checkout main -- file`).
    if let Some(sep) = rest.iter().position(|a| a == "--") {
        let target = if sep > 0 { Some(rest[0].clone()) } else { None };
        let paths = rest[sep + 1..].to_vec();
        return (target, paths);
    }

    // Clap stripped `--`: if we know it was present, all remaining args
    // are paths (no target).
    if has_separator {
        return (None, rest.to_vec());
    }

    // No `--`: first arg is the target, no paths
    (Some(rest[0].clone()), vec![])
}

// ---------------------------------------------------------------------------
// Branch switching
// ---------------------------------------------------------------------------

/// Switch HEAD to an existing branch, updating the working tree and index.
fn switch_branch(
    repo: &Repository,
    branch_name: &str,
    branch_ref: &str,
    force: bool,
) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    // Fail gracefully when HEAD is corrupt (empty or garbage)
    if matches!(head, HeadState::Invalid) {
        bail!("fatal: invalid HEAD - your HEAD file may be corrupt");
    }

    // Check if already on this branch
    if let HeadState::Branch { ref refname, .. } = head {
        if refname == branch_ref {
            checkout_eprintln!("Already on '{}'", branch_name);
            if force {
                // Force mode: reset working tree to match the branch
                let target_oid = refs::resolve_ref(&repo.git_dir, branch_ref)
                    .with_context(|| format!("cannot resolve branch '{branch_name}'"))?;
                let target_tree = commit_to_tree(repo, &target_oid)?;
                return force_reset_to_tree(repo, &target_tree);
            }
            return Ok(());
        }
    }

    let target_oid = refs::resolve_ref(&repo.git_dir, branch_ref)
        .with_context(|| format!("cannot resolve branch '{branch_name}'"))?;

    // If target commit is the same as current HEAD, just re-attach
    // without touching the working tree or index (preserves dirty state).
    // But with -f, always rebuild.
    let already_at_target = head.oid() == Some(&target_oid);
    if !already_at_target || force {
        let target_tree = commit_to_tree(repo, &target_oid)?;

        // Update working tree and index
        switch_to_tree(repo, &head, &target_tree, force)?;
    }

    // Write reflog entries before updating HEAD
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let msg = format!("checkout: moving from {} to {}", from_desc, branch_name);
    write_checkout_reflog(repo, &head, &old_oid, &target_oid, &msg);

    // Update HEAD to point to the branch
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    checkout_eprintln!("Switched to branch '{}'", branch_name);
    Ok(())
}

/// Create a new branch and switch to it.
fn create_and_switch_branch(
    repo: &Repository,
    name: &str,
    start: Option<&str>,
    force: bool,
) -> Result<()> {
    // Check the branch doesn't already exist
    let branch_ref = format!("refs/heads/{name}");
    if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        bail!("a branch named '{}' already exists", name);
    }

    // Resolve start point (default: HEAD)
    let head = resolve_head(&repo.git_dir)?;
    let start_oid = match start {
        Some(s) => resolve_to_commit(repo, s)?,
        None => {
            match head.oid() {
                Some(oid) => *oid,
                None => {
                    // Unborn branch: just switch HEAD to the new branch name
                    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;
                    checkout_eprintln!("Switched to a new branch '{}'", name);
                    return Ok(());
                }
            }
        }
    };

    let target_tree = commit_to_tree(repo, &start_oid)?;

    // Update working tree if start point differs from current HEAD, or if force
    if head.oid() != Some(&start_oid) || force {
        switch_to_tree(repo, &head, &target_tree, force)?;
    }

    // Create the branch ref
    refs::write_ref(&repo.git_dir, &branch_ref, &start_oid)?;

    // Write reflog entries
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let msg = format!("checkout: moving from {} to {}", from_desc, name);
    write_checkout_reflog(repo, &head, &old_oid, &start_oid, &msg);

    // Update HEAD to point to the new branch
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    checkout_eprintln!("Switched to a new branch '{}'", name);
    Ok(())
}

/// Create (or force-reset) a branch and switch to it (`checkout -B`).
fn force_create_and_switch_branch(
    repo: &Repository,
    name: &str,
    start: Option<&str>,
    force: bool,
) -> Result<()> {
    let branch_ref = format!("refs/heads/{name}");
    let branch_existed = refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok();

    // Resolve start point (default: HEAD)
    let start_oid = match start {
        Some(s) => resolve_to_commit(repo, s)?,
        None => {
            let head = resolve_head(&repo.git_dir)?;
            match head.oid() {
                Some(oid) => *oid,
                None => bail!(
                    "cannot create branch '{}': HEAD does not point to a commit",
                    name
                ),
            }
        }
    };

    let head = resolve_head(&repo.git_dir)?;
    let target_tree = commit_to_tree(repo, &start_oid)?;

    // Update working tree if start point differs from current HEAD, or if force
    if head.oid() != Some(&start_oid) || force {
        switch_to_tree(repo, &head, &target_tree, force)?;
    }

    // Write reflog before updating refs
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let msg = format!("checkout: moving from {} to {}", from_desc, name);
    write_checkout_reflog(repo, &head, &old_oid, &start_oid, &msg);

    // Create or overwrite the branch ref
    refs::write_ref(&repo.git_dir, &branch_ref, &start_oid)?;

    // Update HEAD to point to the new branch
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    if branch_existed {
        checkout_eprintln!("Switched to and reset branch '{}'", name);
    } else {
        checkout_eprintln!("Switched to a new branch '{}'", name);
    }
    Ok(())
}

/// Create an orphan branch (`checkout --orphan <name>`).
///
/// Sets HEAD to the new branch but does NOT create the ref (no commit yet).
/// The index is preserved so the next commit will have the current content.
fn create_orphan_branch(repo: &Repository, name: &str) -> Result<()> {
    let branch_ref = format!("refs/heads/{name}");

    // Check the branch doesn't already exist
    if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        bail!("a branch named '{}' already exists", name);
    }

    // Point HEAD at the new branch (which doesn't exist yet = unborn)
    std::fs::write(repo.git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

    checkout_eprintln!("Switched to a new branch '{}'", name);
    Ok(())
}

/// Force-reset working tree to HEAD (`checkout -f` with no arguments).
/// Force-reset the working tree and index to match a given tree object.
fn force_reset_to_tree(repo: &Repository, target_tree: &ObjectId) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => bail!("this operation must be run in a work tree"),
    };

    let old_index = Index::load(&repo.index_path()).unwrap_or_else(|_| Index::new());
    let new_entries = tree_to_flat_entries(repo, target_tree, "")?;
    let mut new_index = Index::new();
    new_index.entries = new_entries;
    new_index.sort();

    // Remove files that are in old index but not in new, and write all entries
    checkout_index_to_worktree(repo, &old_index, &new_index, &work_tree, true)?;

    // Force-write every entry to the worktree
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        write_blob_to_worktree(repo, &work_tree, &path_str, &entry.oid, entry.mode)?;
    }

    new_index
        .write(&repo.index_path())
        .context("writing index")?;
    Ok(())
}

fn force_reset_to_head(repo: &Repository) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => bail!("HEAD does not point to a commit"),
    };
    let target_tree = commit_to_tree(repo, &head_oid)?;

    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => bail!("this operation must be run in a work tree"),
    };

    // Build index from the target tree and force-write all entries
    let new_entries = tree_to_flat_entries(repo, &target_tree, "")?;
    let mut new_index = Index::new();
    new_index.entries = new_entries;
    new_index.sort();

    // Write every entry to the worktree (force overwrite)
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        write_blob_to_worktree(repo, &work_tree, &path_str, &entry.oid, entry.mode)?;
    }

    // Write the new index
    let index_path = repo.index_path();
    new_index.write(&index_path).context("writing index")?;

    // Print current branch/commit info
    match &head {
        HeadState::Branch { refname, .. } => {
            let branch_name = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            checkout_eprintln!("Already on '{}'", branch_name);
        }
        _ => {
            print_detached_head_message(repo, &head_oid)?;
        }
    }
    Ok(())
}

/// Detach HEAD at a specific commit.
fn detach_head(repo: &Repository, oid: &ObjectId, force: bool) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    let already_at_target = head.oid() == Some(oid);
    if !already_at_target || force {
        let target_tree = commit_to_tree(repo, oid)?;
        switch_to_tree(repo, &head, &target_tree, force)?;
    }

    // Write reflog entries
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    let from_desc = match &head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { oid } => oid.to_hex()[..7].to_string(),
        HeadState::Invalid => "unknown".to_string(),
    };
    let to_desc = oid.to_hex()[..7].to_string();
    let msg = format!("checkout: moving from {} to {}", from_desc, to_desc);
    write_checkout_reflog(repo, &head, &old_oid, oid, &msg);

    // Write detached HEAD
    std::fs::write(repo.git_dir.join("HEAD"), format!("{oid}\n"))?;

    print_detached_head_message(repo, oid)?;
    Ok(())
}

/// Switch the working tree and index from the current HEAD tree to a new tree.
///
/// If `force` is false, checks for dirty tracked files that would be overwritten.
fn switch_to_tree(
    repo: &Repository,
    _head: &HeadState,
    target_tree_oid: &ObjectId,
    force: bool,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let index_path = repo.index_path();
    let old_index = Index::load(&index_path).context("loading index")?;

    // Build the new index from the target tree
    let new_entries = tree_to_flat_entries(repo, target_tree_oid, "")?;
    let mut new_index = Index::new();
    new_index.entries = new_entries;
    new_index.sort();

    // Dirty worktree safety check (unless forced)
    if !force {
        check_dirty_worktree(repo, &old_index, &new_index, &work_tree, _head)?;

        // Preserve staged changes: entries in old_index that differ from the
        // HEAD tree and don't conflict with the new tree should be carried
        // through the branch switch.
        let new_paths: HashSet<Vec<u8>> = new_index
            .entries
            .iter()
            .filter(|e| e.stage() == 0)
            .map(|e| e.path.clone())
            .collect();

        let head_tree_oid_map: HashMap<Vec<u8>, ObjectId> =
            (|| -> Result<HashMap<Vec<u8>, ObjectId>> {
                let head_oid = _head.oid().ok_or_else(|| anyhow::anyhow!("no HEAD"))?;
                let head_tree = commit_to_tree(repo, head_oid)?;
                let entries = tree_to_flat_entries(repo, &head_tree, "")?;
                Ok(entries
                    .into_iter()
                    .map(|e| (e.path.clone(), e.oid))
                    .collect())
            })()
            .unwrap_or_default();

        for old_entry in &old_index.entries {
            if old_entry.stage() != 0 {
                continue;
            }

            let in_head = head_tree_oid_map.get(&old_entry.path);
            let is_staged = match in_head {
                Some(hoid) => hoid != &old_entry.oid,
                None => true,
            };
            if !is_staged {
                continue; // index matches HEAD, nothing special to preserve
            }

            if new_paths.contains(&old_entry.path) {
                // The target tree has this file. Check if the target version
                // matches the HEAD version (non-conflicting staged change).
                let target_entry = new_index
                    .entries
                    .iter()
                    .find(|e| e.stage() == 0 && e.path == old_entry.path);
                let target_matches_head = match (target_entry, in_head) {
                    (Some(te), Some(hoid)) => te.oid == *hoid,
                    _ => false,
                };
                if target_matches_head {
                    // Non-conflicting: the target has the same as HEAD.
                    // Preserve the staged version in the new index.
                    new_index.add_or_replace(old_entry.clone());
                }
                // If target differs from HEAD, that's a real conflict
                // (already caught by check_dirty_worktree).
            } else {
                // File not in target tree: preserve staged change.
                new_index.add_or_replace(old_entry.clone());
            }
        }
        new_index.sort();
    }

    // Perform the actual working tree update.
    // When force, write all entries even if OID matches (to restore dirty files).
    checkout_index_to_worktree(repo, &old_index, &new_index, &work_tree, force)?;

    // Write the new index
    new_index.write(&index_path).context("writing index")?;

    Ok(())
}

/// Check if any tracked files have uncommitted changes that would be overwritten
/// by switching to the new index.
fn check_dirty_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    work_tree: &std::path::Path,
    head_state: &HeadState,
) -> Result<()> {
    // Build maps for quick lookup
    let new_map: HashMap<&[u8], &IndexEntry> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    let mut would_overwrite = Vec::new();

    for old_entry in &old_index.entries {
        if old_entry.stage() != 0 {
            continue;
        }

        let path_bytes = &old_entry.path;
        let rel_path = String::from_utf8_lossy(path_bytes);
        let abs_path = work_tree.join(rel_path.as_ref());

        // Check if this file differs between old and new index
        let differs_in_new = match new_map.get(path_bytes.as_slice()) {
            Some(new_entry) => new_entry.oid != old_entry.oid,
            None => true, // file would be deleted
        };

        if !differs_in_new {
            continue;
        }

        // If the file would change, check if the working tree version
        // differs from the current index (i.e., has local modifications)
        if !abs_path.exists() && !abs_path.is_symlink() {
            // File is already gone from worktree, that's fine
            continue;
        }

        // Read the current worktree file and compare with index blob
        if is_worktree_dirty(repo, old_entry, &abs_path)? {
            would_overwrite.push(rel_path.into_owned());
        }
    }

    if !would_overwrite.is_empty() {
        let mut msg = String::from(
            "error: Your local changes to the following files would be overwritten by checkout:\n",
        );
        for path in &would_overwrite {
            msg.push_str(&format!("\t{}\n", path));
        }
        msg.push_str(
            "Please commit your changes or stash them before you switch branches.\nAborting",
        );
        bail!("{}", msg);
    }

    // Check for staged changes that would be lost.
    // A "staged change" means the index entry differs from the HEAD tree.
    // If the target also changes that same file, the checkout must be refused.
    // We need the HEAD tree to detect this.
    {
        // Try to build a map of HEAD tree entries for comparison
        let head_tree_map: HashMap<Vec<u8>, ObjectId> =
            (|| -> Result<HashMap<Vec<u8>, ObjectId>> {
                let head_oid = head_state.oid().ok_or_else(|| anyhow::anyhow!("no HEAD"))?;
                let head_tree = commit_to_tree(repo, head_oid)?;
                let entries = tree_to_flat_entries(repo, &head_tree, "")?;
                Ok(entries
                    .into_iter()
                    .map(|e| (e.path.clone(), e.oid))
                    .collect())
            })()
            .unwrap_or_default();

        if !head_tree_map.is_empty() {
            let mut staged_conflicts = Vec::new();
            for old_entry in &old_index.entries {
                if old_entry.stage() != 0 {
                    continue;
                }
                let path_bytes = &old_entry.path;
                // Check if index differs from HEAD (i.e., file is staged)
                let head_oid = head_tree_map.get(path_bytes);
                let is_staged = match head_oid {
                    Some(hoid) => hoid != &old_entry.oid,
                    None => true, // new file in index = staged addition
                };
                if !is_staged {
                    continue;
                }
                // Check if the target also changes this file
                // Check if the staged content differs from the target.
                // A real conflict exists only when:
                // 1. The file is staged (index ≠ HEAD) — checked above
                // 2. The target also changes the file (target ≠ HEAD)
                // 3. The staged content differs from the target (index ≠ target)
                let new_entry = new_map.get(path_bytes.as_slice());

                // If staged content matches the target, no data loss.
                let staged_matches_target = match new_entry {
                    Some(ne) => ne.oid == old_entry.oid,
                    None => false,
                };
                if staged_matches_target {
                    continue;
                }

                // Check if the target actually changes this file from HEAD
                let target_changes = match (head_oid, new_entry) {
                    (Some(hoid), Some(ne)) => ne.oid != *hoid,
                    (Some(_), None) => true, // target removes the file
                    (None, Some(_)) => true, // target adds a file we also added
                    (None, None) => false,   // neither HEAD nor target have it
                };
                if !target_changes {
                    continue; // target doesn't touch this file, staged change is safe
                }

                // Both staged and target change the file from HEAD.
                // If the target removes the file but we have staged changes,
                // git allows this (file is preserved in index/worktree).
                // Only block if target adds/changes to different content.
                if new_entry.is_none() && head_oid.is_some() {
                    continue; // target removes, but our staged version can be carried
                }

                // The index differs from both HEAD and the target, so
                // switching would silently discard the staged change.
                let rel_path = String::from_utf8_lossy(path_bytes);
                staged_conflicts.push(rel_path.into_owned());
            }
            if !staged_conflicts.is_empty() {
                let mut msg = String::from("error: Your local changes to the following files would be overwritten by checkout:\n");
                for path in &staged_conflicts {
                    msg.push_str(&format!("\t{}\n", path));
                }
                msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
                bail!("{}", msg);
            }
        }
    }

    // Check for untracked files that would be overwritten by new entries.
    // Include all stages (not just stage 0) so that files in a merge conflict
    // (which only have higher-stage entries) are still recognized as tracked.
    let old_paths: HashSet<&[u8]> = old_index
        .entries
        .iter()
        .map(|e| e.path.as_slice())
        .collect();

    let mut untracked_conflicts = Vec::new();
    for new_entry in &new_index.entries {
        if new_entry.stage() != 0 {
            continue;
        }
        // If this path is not in the old index, it's a new file from the target.
        // Check if an untracked file exists at that path.
        if !old_paths.contains(new_entry.path.as_slice()) {
            let rel_path = String::from_utf8_lossy(&new_entry.path);
            let abs_path = work_tree.join(rel_path.as_ref());
            if abs_path.exists() || abs_path.is_symlink() {
                // Before flagging as untracked, check if the path only exists
                // because of a tracked symlink or tracked directory in the old
                // index. E.g. switching from a branch with symlink `frotz` to
                // one with directory `frotz/` — `frotz/filfre` resolves through
                // the tracked symlink and is not truly untracked.
                let rel_str = rel_path.as_ref();

                // Case 1: A parent component of the new path is a tracked
                // entry (symlink) in the old index.
                let has_tracked_prefix = rel_str.find('/').is_some_and(|_| {
                    let mut prefix = String::new();
                    for component in rel_str.split('/') {
                        if !prefix.is_empty() {
                            prefix.push('/');
                        }
                        prefix.push_str(component);
                        if prefix.len() < rel_str.len() && old_paths.contains(prefix.as_bytes()) {
                            return true;
                        }
                    }
                    false
                });

                // Case 2: The new entry replaces a directory that contains
                // tracked files (dir→symlink transition). Check if any old
                // tracked path starts with this entry's path as a directory
                // prefix.
                let replaces_tracked_dir = old_paths.iter().any(|op| {
                    let op_str = String::from_utf8_lossy(op);
                    op_str.starts_with(rel_str)
                        && op_str.as_bytes().get(rel_str.len()) == Some(&b'/')
                });

                if !has_tracked_prefix && !replaces_tracked_dir {
                    untracked_conflicts.push(rel_path.into_owned());
                }
            }
        }
    }

    if !untracked_conflicts.is_empty() {
        let mut msg = String::from(
            "error: The following untracked working tree files would be overwritten by checkout:\n",
        );
        for path in &untracked_conflicts {
            msg.push_str(&format!("\t{}\n", path));
        }
        msg.push_str("Please move or remove them before you switch branches.\nAborting");
        bail!("{}", msg);
    }

    Ok(())
}

/// Check if a working tree file differs from its index entry.
fn is_worktree_dirty(
    repo: &Repository,
    entry: &IndexEntry,
    abs_path: &std::path::Path,
) -> Result<bool> {
    if entry.mode == MODE_SYMLINK {
        // For symlinks, compare the target
        match std::fs::read_link(abs_path) {
            Ok(target) => {
                let obj = repo.odb.read(&entry.oid)?;
                let expected = String::from_utf8_lossy(&obj.data);
                Ok(target.to_string_lossy() != expected.as_ref())
            }
            Err(_) => Ok(true),
        }
    } else {
        // For regular files, compare content
        match std::fs::read(abs_path) {
            Ok(data) => {
                let obj = repo.odb.read(&entry.oid)?;
                Ok(data != obj.data)
            }
            Err(_) => Ok(true),
        }
    }
}

// ---------------------------------------------------------------------------
// Path-based checkout (restore files)
// ---------------------------------------------------------------------------

/// Checkout specific paths from the index or a tree-ish.
fn checkout_paths(
    repo: &Repository,
    source: Option<&str>,
    paths: &[String],
    no_overlay: bool,
) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cwd = std::env::current_dir().context("resolving cwd")?;

    match source {
        None => {
            // checkout -- <paths>: restore from index
            let index_path = repo.index_path();
            let index = Index::load(&index_path).context("loading index")?;

            for path_str in paths {
                let rel = resolve_pathspec(path_str, work_tree, &cwd);
                let path_bytes = rel.as_bytes();

                // Handle glob pathspecs
                if is_glob_pattern(&rel) {
                    let mut matched = false;
                    for ie in &index.entries {
                        if ie.stage() != 0 {
                            continue;
                        }
                        let p = String::from_utf8_lossy(&ie.path).to_string();
                        if glob_matches(&rel, &p) {
                            write_blob_to_worktree(repo, work_tree, &p, &ie.oid, ie.mode)?;
                            matched = true;
                        }
                    }
                    if !matched {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                    continue;
                }

                // Handle directory pathspecs (including "." for repo root)
                let is_root = rel.is_empty() || rel == ".";
                if is_root {
                    // Restore ALL index entries
                    for ie in &index.entries {
                        if ie.stage() != 0 {
                            continue;
                        }
                        let p = String::from_utf8_lossy(&ie.path).to_string();
                        write_blob_to_worktree(repo, work_tree, &p, &ie.oid, ie.mode)?;
                    }
                } else if let Some(entry) = index.get(path_bytes, 0) {
                    // Exact file match
                    write_blob_to_worktree(repo, work_tree, &rel, &entry.oid, entry.mode)?;
                } else {
                    // Try as a directory prefix
                    let prefix = if rel.ends_with('/') {
                        rel.clone()
                    } else {
                        format!("{rel}/")
                    };
                    let mut matched = false;
                    for ie in &index.entries {
                        if ie.stage() != 0 {
                            continue;
                        }
                        let p = String::from_utf8_lossy(&ie.path).to_string();
                        if p.starts_with(&prefix) {
                            write_blob_to_worktree(repo, work_tree, &p, &ie.oid, ie.mode)?;
                            matched = true;
                        }
                    }
                    if !matched {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                }
            }
        }
        Some(source_spec) => {
            // checkout <commit> -- <paths>: restore from a specific commit's tree
            let source_oid = resolve_to_commit(repo, source_spec)?;
            let tree_oid = commit_to_tree(repo, &source_oid)?;

            let index_path = repo.index_path();
            let mut index = Index::load(&index_path).context("loading index")?;
            let mut index_modified = false;

            for path_str in paths {
                let rel = resolve_pathspec(path_str, work_tree, &cwd);

                // Handle glob pathspecs
                if is_glob_pattern(&rel) {
                    let flat = tree_to_flat_entries(repo, &tree_oid, "")?;
                    let source_paths: HashSet<Vec<u8>> = flat
                        .iter()
                        .filter(|e| {
                            let p = String::from_utf8_lossy(&e.path);
                            glob_matches(&rel, &p)
                        })
                        .map(|e| e.path.clone())
                        .collect();
                    let mut matched = false;
                    for flat_entry in &flat {
                        let entry_path = String::from_utf8_lossy(&flat_entry.path).to_string();
                        if !glob_matches(&rel, &entry_path) {
                            continue;
                        }
                        write_blob_to_worktree(
                            repo,
                            work_tree,
                            &entry_path,
                            &flat_entry.oid,
                            flat_entry.mode,
                        )?;
                        index.add_or_replace(flat_entry.clone());
                        index_modified = true;
                        matched = true;
                    }
                    if no_overlay {
                        let to_remove: Vec<Vec<u8>> = index
                            .entries
                            .iter()
                            .filter(|e| e.stage() == 0)
                            .filter(|e| {
                                let p = String::from_utf8_lossy(&e.path);
                                glob_matches(&rel, &p)
                            })
                            .filter(|e| !source_paths.contains(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        for path in &to_remove {
                            let p = String::from_utf8_lossy(path);
                            let abs = work_tree.join(p.as_ref());
                            let _ = std::fs::remove_file(&abs);
                            remove_empty_parent_dirs(work_tree, &abs);
                        }
                        index.entries.retain(|e| {
                            if e.stage() != 0 {
                                return true;
                            }
                            !to_remove.contains(&e.path)
                        });
                        if !to_remove.is_empty() {
                            index_modified = true;
                        }
                        matched = matched || !to_remove.is_empty();
                    }
                    if !matched {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                    continue;
                }

                // Check if this is a directory prefix or empty ("."/root)
                let is_dir_prefix = rel.is_empty() || {
                    // Check if the path is a tree (directory) in the source
                    match find_in_tree(repo, tree_oid, &rel)? {
                        Some((_, mode)) if mode == 0o40000 => true,
                        Some(_) => false,
                        None => rel.is_empty(),
                    }
                };

                if is_dir_prefix {
                    // Restore all files under this directory from the source tree
                    let flat = tree_to_flat_entries(repo, &tree_oid, "")?;
                    let prefix = if rel.is_empty() {
                        String::new()
                    } else if rel.ends_with('/') {
                        rel.clone()
                    } else {
                        format!("{}/", rel)
                    };
                    let source_paths: HashSet<Vec<u8>> = flat
                        .iter()
                        .filter(|e| {
                            prefix.is_empty()
                                || String::from_utf8_lossy(&e.path).starts_with(&prefix)
                        })
                        .map(|e| e.path.clone())
                        .collect();
                    let mut matched = false;
                    for flat_entry in &flat {
                        let entry_path = String::from_utf8_lossy(&flat_entry.path).to_string();
                        if !prefix.is_empty() && !entry_path.starts_with(&prefix) {
                            continue;
                        }
                        write_blob_to_worktree(
                            repo,
                            work_tree,
                            &entry_path,
                            &flat_entry.oid,
                            flat_entry.mode,
                        )?;
                        index.add_or_replace(flat_entry.clone());
                        index_modified = true;
                        matched = true;
                    }
                    // In no-overlay mode, remove index entries that match the
                    // pathspec but are NOT in the source tree.
                    if no_overlay {
                        let to_remove: Vec<Vec<u8>> = index
                            .entries
                            .iter()
                            .filter(|e| e.stage() == 0)
                            .filter(|e| {
                                if prefix.is_empty() {
                                    true
                                } else {
                                    String::from_utf8_lossy(&e.path).starts_with(&prefix)
                                }
                            })
                            .filter(|e| !source_paths.contains(&e.path))
                            .map(|e| e.path.clone())
                            .collect();
                        for path in &to_remove {
                            let p = String::from_utf8_lossy(path);
                            let abs = work_tree.join(p.as_ref());
                            let _ = std::fs::remove_file(&abs);
                            remove_empty_parent_dirs(work_tree, &abs);
                        }
                        index.entries.retain(|e| {
                            if e.stage() != 0 {
                                return true;
                            }
                            !to_remove.contains(&e.path)
                        });
                        if !to_remove.is_empty() {
                            index_modified = true;
                        }
                        matched = matched || !to_remove.is_empty();
                    }
                    if !matched && source_paths.is_empty() {
                        bail!(
                            "error: pathspec '{}' did not match any file(s) known to git",
                            path_str
                        );
                    }
                } else {
                    let (blob_oid, mode) =
                        find_in_tree(repo, tree_oid, &rel)?.ok_or_else(|| {
                            anyhow::anyhow!(
                                "error: pathspec '{}' did not match any file(s) known to git",
                                path_str
                            )
                        })?;

                    // Write to working tree with CRLF conversion
                    write_blob_to_worktree(repo, work_tree, &rel, &blob_oid, mode)?;

                    // Read blob size for index entry
                    let obj = repo
                        .odb
                        .read(&blob_oid)
                        .with_context(|| format!("reading blob for '{rel}'"))?;

                    // Update index entry
                    let path_bytes = rel.as_bytes().to_vec();
                    let entry = IndexEntry {
                        ctime_sec: 0,
                        ctime_nsec: 0,
                        mtime_sec: 0,
                        mtime_nsec: 0,
                        dev: 0,
                        ino: 0,
                        mode,
                        uid: 0,
                        gid: 0,
                        size: obj.data.len() as u32,
                        oid: blob_oid,
                        flags: path_bytes.len().min(0xFFF) as u16,
                        flags_extended: None,
                        path: path_bytes,
                    };
                    index.add_or_replace(entry);
                    index_modified = true;
                }
            }

            if index_modified {
                index.write(&index_path).context("writing index")?;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive patch mode
// ---------------------------------------------------------------------------

/// Interactive patch-mode checkout (`checkout -p`).
///
/// Shows each hunk of difference between the source (index or commit) and the
/// working tree, prompting the user to accept (y), reject (n), quit (q),
/// accept-all-in-file (a), or skip-rest-of-file (d) for each hunk.
fn checkout_patch(repo: &Repository, source: Option<&str>, paths: &[String]) -> Result<()> {
    use similar::TextDiff;
    use std::io::{self, BufRead, Write};

    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?;

    let cwd = std::env::current_dir().context("resolving cwd")?;
    let index_path = repo.index_path();
    let index = Index::load(&index_path).context("loading index")?;

    // Determine which files to consider
    let filter_paths: Vec<String> = paths
        .iter()
        .map(|p| resolve_pathspec(p, work_tree, &cwd))
        .collect();

    // Build list of (rel_path, source_content) pairs for modified files
    let mut file_diffs: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new(); // (path, source_bytes, worktree_bytes)

    match source {
        None => {
            // Diff working tree against index
            for ie in &index.entries {
                if ie.stage() != 0 {
                    continue;
                }
                if ie.mode == MODE_SYMLINK {
                    continue;
                }

                let path_str = String::from_utf8_lossy(&ie.path).to_string();

                // Apply path filter if specified
                if !filter_paths.is_empty() {
                    let matches = filter_paths.iter().any(|fp| {
                        if is_glob_pattern(fp) {
                            glob_matches(fp, &path_str)
                        } else if fp.is_empty() || fp == "." {
                            true
                        } else if fp.ends_with('/') {
                            path_str.starts_with(fp.as_str())
                        } else {
                            path_str == *fp || path_str.starts_with(&format!("{fp}/"))
                        }
                    });
                    if !matches {
                        continue;
                    }
                }

                let abs_path = work_tree.join(&path_str);
                if !abs_path.exists() {
                    // Deleted file — treat as empty worktree content
                    let obj = repo.odb.read(&ie.oid)?;
                    if obj.kind == ObjectKind::Blob {
                        file_diffs.push((path_str, obj.data.clone(), Vec::new()));
                    }
                    continue;
                }

                let worktree_data =
                    std::fs::read(&abs_path).with_context(|| format!("reading {path_str}"))?;
                let obj = repo.odb.read(&ie.oid)?;
                if obj.kind != ObjectKind::Blob {
                    continue;
                }

                if worktree_data != obj.data {
                    file_diffs.push((path_str, obj.data.clone(), worktree_data));
                }
            }
        }
        Some(source_spec) => {
            // Diff working tree against a specific commit's tree
            let source_oid = resolve_to_commit(repo, source_spec)?;
            let tree_oid = commit_to_tree(repo, &source_oid)?;
            let flat = tree_to_flat_entries(repo, &tree_oid, "")?;

            for flat_entry in &flat {
                if flat_entry.mode == MODE_SYMLINK {
                    continue;
                }
                let path_str = String::from_utf8_lossy(&flat_entry.path).to_string();

                if !filter_paths.is_empty() {
                    let matches = filter_paths.iter().any(|fp| {
                        if is_glob_pattern(fp) {
                            glob_matches(fp, &path_str)
                        } else if fp.is_empty() || fp == "." {
                            true
                        } else {
                            path_str == *fp || path_str.starts_with(&format!("{fp}/"))
                        }
                    });
                    if !matches {
                        continue;
                    }
                }

                let abs_path = work_tree.join(&path_str);
                let worktree_data = if abs_path.exists() {
                    std::fs::read(&abs_path).with_context(|| format!("reading {path_str}"))?
                } else {
                    Vec::new()
                };

                let obj = repo.odb.read(&flat_entry.oid)?;
                if obj.kind != ObjectKind::Blob {
                    continue;
                }

                if worktree_data != obj.data {
                    file_diffs.push((path_str, obj.data.clone(), worktree_data));
                }
            }
        }
    }

    if file_diffs.is_empty() {
        return Ok(());
    }

    // Sort for deterministic order
    file_diffs.sort_by(|a, b| a.0.cmp(&b.0));

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stderr();

    for (path, source_data, worktree_data) in &file_diffs {
        let source_str = String::from_utf8_lossy(source_data);
        let worktree_str = String::from_utf8_lossy(worktree_data);

        // The diff shows what changed FROM source TO worktree.
        // "Accepting" a hunk means reverting the worktree back to source.
        let text_diff = TextDiff::from_lines(source_str.as_ref(), worktree_str.as_ref());
        let hunks: Vec<_> = text_diff
            .unified_diff()
            .context_radius(3)
            .iter_hunks()
            .collect();

        if hunks.is_empty() {
            continue;
        }

        let mut accept_all = false;
        let mut skip_file = false;
        let mut accepted_hunks: Vec<bool> = vec![false; hunks.len()];

        for (i, hunk) in hunks.iter().enumerate() {
            if skip_file {
                break;
            }
            if accept_all {
                accepted_hunks[i] = true;
                continue;
            }

            // Display the hunk
            writeln!(stdout, "diff --git a/{path} b/{path}").ok();
            write!(stdout, "--- a/{path}\n+++ b/{path}\n").ok();
            write!(stdout, "{hunk}").ok();
            write!(stdout, "Discard this hunk from worktree [y,n,q,a,d,?]? ").ok();
            stdout.flush().ok();

            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                // EOF — keep remaining changes
                break;
            }
            let answer = line.trim();
            match answer {
                "y" | "Y" => {
                    accepted_hunks[i] = true;
                }
                "n" | "N" => { /* keep this hunk (don't revert) */ }
                "a" | "A" => {
                    accepted_hunks[i] = true;
                    accept_all = true;
                }
                "d" | "D" => {
                    skip_file = true;
                }
                "q" | "Q" => {
                    // Apply what we've accepted so far, then return
                    apply_accepted_hunks(
                        repo,
                        work_tree,
                        path,
                        source_data,
                        worktree_data,
                        &accepted_hunks,
                    )?;
                    return Ok(());
                }
                _ => { /* Unrecognized — treat as 'n' */ }
            }
        }

        // Apply accepted hunks for this file
        apply_accepted_hunks(
            repo,
            work_tree,
            path,
            source_data,
            worktree_data,
            &accepted_hunks,
        )?;
    }

    Ok(())
}

/// Apply accepted hunks by reconstructing the file content.
///
/// For each accepted hunk, we revert the worktree lines back to the source
/// version. Unaccepted hunks keep the worktree version.
fn apply_accepted_hunks(
    _repo: &Repository,
    work_tree: &std::path::Path,
    path: &str,
    source_data: &[u8],
    worktree_data: &[u8],
    accepted: &[bool],
) -> Result<()> {
    if !accepted.iter().any(|&a| a) {
        return Ok(()); // Nothing accepted
    }

    let abs_path = work_tree.join(path);

    // If all hunks are accepted, just write the source content
    if accepted.iter().all(|&a| a) {
        if source_data.is_empty() {
            let _ = std::fs::remove_file(&abs_path);
        } else {
            if let Some(parent) = abs_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&abs_path, source_data)?;
        }
        return Ok(());
    }

    // Partial acceptance: reconstruct file using diff ops.
    // Each non-Equal op is a "hunk". We map each contiguous group of
    // non-Equal ops to the same hunk index.
    let source_str = String::from_utf8_lossy(source_data);
    let worktree_str = String::from_utf8_lossy(worktree_data);
    let source_lines: Vec<&str> = source_str.lines().collect();
    let worktree_lines: Vec<&str> = worktree_str.lines().collect();

    let text_diff = similar::TextDiff::from_lines(source_str.as_ref(), worktree_str.as_ref());

    // Map ops to hunk indices: consecutive non-Equal ops share a hunk index.
    let ops: Vec<_> = text_diff.ops().to_vec();
    let mut hunk_indices: Vec<usize> = Vec::new();
    let mut current_hunk: usize = 0;
    let mut prev_was_change = false;
    for op in &ops {
        match op {
            similar::DiffOp::Equal { .. } => {
                hunk_indices.push(usize::MAX); // sentinel for equal
                if prev_was_change {
                    current_hunk += 1;
                    prev_was_change = false;
                }
            }
            _ => {
                hunk_indices.push(current_hunk);
                prev_was_change = true;
            }
        }
    }

    let mut output = String::new();
    for (i, op) in ops.iter().enumerate() {
        let hi = hunk_indices[i];
        let is_accepted = hi != usize::MAX && hi < accepted.len() && accepted[hi];

        match op {
            similar::DiffOp::Equal { old_index, len, .. } => {
                for j in 0..*len {
                    output.push_str(source_lines[old_index + j]);
                    output.push('\n');
                }
            }
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => {
                if is_accepted {
                    // Restore source lines
                    for j in 0..*old_len {
                        output.push_str(source_lines[old_index + j]);
                        output.push('\n');
                    }
                }
                // Rejected: lines stay deleted
            }
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => {
                if !is_accepted {
                    // Keep worktree additions
                    for j in 0..*new_len {
                        output.push_str(worktree_lines[new_index + j]);
                        output.push('\n');
                    }
                }
                // Accepted: revert additions (don't include them)
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                if is_accepted {
                    // Restore source lines
                    for j in 0..*old_len {
                        output.push_str(source_lines[old_index + j]);
                        output.push('\n');
                    }
                } else {
                    // Keep worktree lines
                    for j in 0..*new_len {
                        output.push_str(worktree_lines[new_index + j]);
                        output.push('\n');
                    }
                }
            }
        }
    }

    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs_path, output.as_bytes())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Output messages
// ---------------------------------------------------------------------------

/// Print detached HEAD message.
fn print_detached_head_message(repo: &Repository, oid: &ObjectId) -> Result<()> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Commit {
        return Ok(());
    }
    let commit = parse_commit(&obj.data)?;
    let subject = commit.message.lines().next().unwrap_or("").trim();
    let abbrev =
        abbreviate_object_id(repo, *oid, 7).unwrap_or_else(|_| oid.to_hex()[..7].to_owned());

    // Print detached HEAD advice unless advice.detachedHead is false
    let show_advice = match ConfigSet::load(Some(&repo.git_dir), true) {
        Ok(config) => match config.get_bool("advice.detachedHead") {
            Some(Ok(val)) => val,
            _ => true, // default: show advice
        },
        Err(_) => true,
    };
    if show_advice {
        checkout_eprintln!(
            "Note: switching to '{}'.\n\
             \n\
             You are in 'detached HEAD' state. You can look around, make experimental\n\
             changes and commit them, and you can discard any commits you make in this\n\
             state without impacting any branches by switching back to a branch.\n\
             \n\
             If you want to create a new branch to retain commits you create, you may\n\
             do so (now or later) by using -c with the switch command. Example:\n\
             \n\
               git switch -c <new-branch-name>\n\
             \n\
             Or undo this operation with:\n\
             \n\
               git switch -\n\
             \n\
             Turn off this advice by setting config variable advice.detachedHead to false\n",
            oid
        );
    }

    checkout_eprintln!("HEAD is now at {} {}", abbrev, subject);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tracking (upstream) configuration
// ---------------------------------------------------------------------------

/// Set up tracking configuration for a newly created branch.
///
/// With `--track`, sets `branch.<name>.remote` and `branch.<name>.merge`.
/// Also respects `branch.autoSetupMerge` config.
fn maybe_setup_tracking(
    repo: &Repository,
    branch_name: &str,
    start_point: Option<&str>,
    explicit_track: bool,
) -> Result<()> {
    let start = match start_point {
        Some(s) => s,
        None => return Ok(()), // no start point → nothing to track
    };

    // Check if auto-setup is enabled
    if !explicit_track {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let auto = config.get("branch.autoSetupMerge").unwrap_or_default();
        match auto.as_str() {
            "always" => {} // proceed
            "false" | "never" => return Ok(()),
            _ => {
                // Default ("true"): only auto-track remote branches
                // For local branches, only track if --track was explicit
                // Since we don't have remote tracking branches yet, this
                // means we only track local branches with explicit --track
                return Ok(());
            }
        }
    }

    // Check if start_point is a local branch
    let start_ref = format!("refs/heads/{start}");
    if refs::resolve_ref(&repo.git_dir, &start_ref).is_ok() {
        // Set up tracking for a local branch
        let config_path = repo.git_dir.join("config");
        let mut config_content = std::fs::read_to_string(&config_path).unwrap_or_default();

        let section = format!(
            "\n[branch \"{}\"]\
            \n\tremote = .\
            \n\tmerge = {}\n",
            branch_name, start_ref
        );
        config_content.push_str(&section);
        std::fs::write(&config_path, config_content)?;

        checkout_eprintln!("branch '{}' set up to track '{}'.", branch_name, start);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tree / object helpers (local to this command)
// ---------------------------------------------------------------------------

/// Resolve a revision spec to a commit OID, peeling through tags.
fn resolve_to_commit(repo: &Repository, spec: &str) -> Result<ObjectId> {
    let oid =
        resolve_revision(repo, spec).with_context(|| format!("unknown revision: '{spec}'"))?;
    peel_to_commit(repo, oid)
}

/// Peel an OID to a commit (follows tag chains).
fn peel_to_commit(repo: &Repository, mut oid: ObjectId) -> Result<ObjectId> {
    for _ in 0..10 {
        let obj = repo.odb.read(&oid)?;
        match obj.kind {
            ObjectKind::Commit => return Ok(oid),
            ObjectKind::Tag => {
                let text = std::str::from_utf8(&obj.data).context("tag is not UTF-8")?;
                let target_hex = text
                    .lines()
                    .find_map(|l| l.strip_prefix("object "))
                    .ok_or_else(|| anyhow::anyhow!("tag missing 'object' header"))?
                    .trim();
                oid = target_hex.parse()?;
            }
            _ => bail!("'{}' is not a commit-ish", oid),
        }
    }
    bail!("too many levels of tag dereferencing")
}

/// Extract the tree OID from a commit object.
fn commit_to_tree(repo: &Repository, commit_oid: &ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(commit_oid)?;
    if obj.kind != ObjectKind::Commit {
        bail!("not a commit: {commit_oid}");
    }
    let commit = parse_commit(&obj.data)?;
    Ok(commit.tree)
}

/// Recursively flatten a tree object into a list of [`IndexEntry`] values.
fn tree_to_flat_entries(
    repo: &Repository,
    tree_oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<IndexEntry>> {
    let obj = repo.odb.read(tree_oid)?;
    if obj.kind != ObjectKind::Tree {
        bail!("expected tree, got {}", obj.kind);
    }
    let entries = parse_tree(&obj.data)?;
    let mut result = Vec::new();

    for te in entries {
        let name = String::from_utf8_lossy(&te.name).into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };

        if te.mode == 0o040000 {
            result.extend(tree_to_flat_entries(repo, &te.oid, &path)?);
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

/// Walk a tree to find the blob (OID, mode) at `path` (slash-separated).
fn find_in_tree(
    repo: &Repository,
    tree_oid: ObjectId,
    path: &str,
) -> Result<Option<(ObjectId, u32)>> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    find_recursive(repo, tree_oid, &parts)
}

fn find_recursive(
    repo: &Repository,
    tree_oid: ObjectId,
    parts: &[&str],
) -> Result<Option<(ObjectId, u32)>> {
    if parts.is_empty() {
        return Ok(None);
    }

    let tree_obj = repo
        .odb
        .read(&tree_oid)
        .with_context(|| format!("reading tree {tree_oid}"))?;
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(None);
    }

    let entries = parse_tree(&tree_obj.data)?;
    let name_bytes = parts[0].as_bytes();
    let Some(entry) = entries.iter().find(|e| e.name == name_bytes) else {
        return Ok(None);
    };

    if parts.len() == 1 {
        Ok(Some((entry.oid, entry.mode)))
    } else {
        find_recursive(repo, entry.oid, &parts[1..])
    }
}

// ---------------------------------------------------------------------------
// Working tree helpers
// ---------------------------------------------------------------------------

/// Update the working tree from old_index to new_index: remove deleted files,
/// add new files, update modified files.
fn checkout_index_to_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &Index,
    work_tree: &std::path::Path,
    force_write_all: bool,
) -> Result<()> {
    let old_stage0: HashSet<Vec<u8>> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();
    let new_stage0: HashSet<Vec<u8>> = new_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| e.path.clone())
        .collect();

    // Build old index map for OID comparison
    let old_map: HashMap<&[u8], &IndexEntry> = old_index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.as_slice(), e))
        .collect();

    // Remove paths that are no longer present in the new index.
    for old_path in old_stage0.difference(&new_stage0) {
        let rel = String::from_utf8_lossy(old_path).into_owned();
        let abs = work_tree.join(&rel);
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        } else if abs.is_dir() {
            let _ = std::fs::remove_dir_all(&abs);
        }
        remove_empty_parent_dirs(work_tree, &abs);
    }

    // Write new/modified entries
    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }

        // Skip gitlink (submodule) entries — their OIDs reference commits
        // in the submodule's object store, not blobs in ours.
        if entry.mode == 0o160000 {
            // Ensure the submodule directory exists so that scripts can
            // `cd` into it, but don't try to check out any content.
            let sm_dir = work_tree.join(String::from_utf8_lossy(&entry.path).as_ref());
            let _ = std::fs::create_dir_all(&sm_dir);
            continue;
        }

        // Skip unchanged entries (same OID and mode) — but only if file exists
        // and we're not in force mode.
        if !force_write_all {
            if let Some(old_entry) = old_map.get(entry.path.as_slice()) {
                if old_entry.oid == entry.oid && old_entry.mode == entry.mode {
                    let abs_path = work_tree.join(String::from_utf8_lossy(&entry.path).as_ref());
                    if abs_path.exists() || abs_path.is_symlink() {
                        continue;
                    }
                    // File was deleted from worktree, restore it
                }
            }
        }

        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        write_blob_to_worktree(repo, work_tree, &path_str, &entry.oid, entry.mode)?;
    }

    Ok(())
}

/// Write a blob object to the working tree.
fn write_blob_to_worktree(
    repo: &Repository,
    work_tree: &std::path::Path,
    rel_path: &str,
    oid: &ObjectId,
    mode: u32,
) -> Result<()> {
    let obj = repo.odb.read(oid).context("reading object for checkout")?;
    if obj.kind != ObjectKind::Blob {
        bail!("cannot checkout non-blob at '{rel_path}'");
    }

    // Apply CRLF / smudge conversion for checkout
    let data = if mode != MODE_SYMLINK {
        let config = ConfigSet::load(Some(&repo.git_dir), true).unwrap_or_default();
        let conv = crlf::ConversionConfig::from_config(&config);
        let mut attrs = crlf::load_gitattributes(work_tree);
        if attrs.is_empty() {
            // Try loading from the index (during checkout when worktree may not have .gitattributes yet)
            if let Ok(idx) = Index::load(&repo.index_path()) {
                attrs = crlf::load_gitattributes_from_index(&idx, &repo.odb);
            }
        }
        let file_attrs = crlf::get_file_attrs(&attrs, rel_path, &config);
        let oid_hex = format!("{oid}");
        crlf::convert_to_worktree(&obj.data, rel_path, &conv, &file_attrs, Some(&oid_hex))
    } else {
        obj.data
    };

    write_to_worktree(work_tree, rel_path, &data, mode)
}

/// Write data to a working tree file, handling symlinks and executable bits.
fn write_to_worktree(
    work_tree: &std::path::Path,
    rel_path: &str,
    data: &[u8],
    mode: u32,
) -> Result<()> {
    let abs_path = work_tree.join(rel_path);

    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent directories for '{rel_path}'"))?;
    }

    // Remove existing file/dir at target path
    if abs_path.exists() || std::fs::symlink_metadata(&abs_path).is_ok() {
        if abs_path.is_dir() {
            std::fs::remove_dir_all(&abs_path)?;
        } else {
            std::fs::remove_file(&abs_path)?;
        }
    }

    if mode == MODE_SYMLINK {
        let target = std::str::from_utf8(data)
            .with_context(|| format!("symlink target for '{rel_path}' is not UTF-8"))?;
        std::os::unix::fs::symlink(target, &abs_path)
            .with_context(|| format!("creating symlink '{rel_path}'"))?;
    } else {
        std::fs::write(&abs_path, data).with_context(|| format!("writing '{rel_path}'"))?;

        if mode == MODE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&abs_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&abs_path, perms)?;
        }
    }

    Ok(())
}

/// Remove empty parent directories up to (but not including) `work_tree`.
fn remove_empty_parent_dirs(work_tree: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == work_tree {
            break;
        }
        match std::fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

/// Check if a pathspec contains glob characters.
fn is_glob_pattern(spec: &str) -> bool {
    spec.contains('*') || spec.contains('?') || spec.contains('[')
}

/// Match a path against a simple glob pattern.
/// Supports `*` (any chars except `/`), `?` (any single char except `/`),
/// and character classes `[abc]`.
fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_matches_inner(pattern.as_bytes(), path.as_bytes())
}

fn glob_matches_inner(pattern: &[u8], path: &[u8]) -> bool {
    let mut pi = 0; // pattern index
    let mut si = 0; // string index
    let mut star_pi = usize::MAX;
    let mut star_si = 0;

    while si < path.len() {
        if pi < pattern.len() && pattern[pi] == b'?' && path[si] != b'/' {
            pi += 1;
            si += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            if pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
                // "**" matches everything including '/'
                // For simplicity, try matching rest of pattern at every position
                let rest = &pattern[pi + 2..];
                // Skip optional '/' after **
                let rest = if !rest.is_empty() && rest[0] == b'/' {
                    &rest[1..]
                } else {
                    rest
                };
                for i in si..=path.len() {
                    if glob_matches_inner(rest, &path[i..]) {
                        return true;
                    }
                }
                return false;
            }
            star_pi = pi;
            star_si = si;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == b'[' {
            // Character class
            pi += 1;
            let negate = pi < pattern.len() && (pattern[pi] == b'!' || pattern[pi] == b'^');
            if negate {
                pi += 1;
            }
            let mut found = false;
            let ch = path[si];
            while pi < pattern.len() && pattern[pi] != b']' {
                if pi + 2 < pattern.len() && pattern[pi + 1] == b'-' {
                    if ch >= pattern[pi] && ch <= pattern[pi + 2] {
                        found = true;
                    }
                    pi += 3;
                } else {
                    if ch == pattern[pi] {
                        found = true;
                    }
                    pi += 1;
                }
            }
            if pi < pattern.len() {
                pi += 1;
            } // skip ']'
            if found == negate {
                // Mismatch in character class
                if star_pi != usize::MAX {
                    pi = star_pi + 1;
                    star_si += 1;
                    si = star_si;
                } else {
                    return false;
                }
            } else {
                si += 1;
            }
        } else if pi < pattern.len() && pattern[pi] == path[si] {
            pi += 1;
            si += 1;
        } else if star_pi != usize::MAX && path[si] != b'/' {
            // Backtrack: '*' matches one more character (but not '/')
            pi = star_pi + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    // Consume trailing '*' or '**' in pattern
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Resolve a pathspec to a repository-relative path.
fn resolve_pathspec(spec: &str, work_tree: &Path, cwd: &Path) -> String {
    // Handle :/ prefix (repo root)
    if spec == ":/" || spec.starts_with(":/") {
        let rest = &spec[2..];
        return rest.to_owned();
    }

    let candidate = std::path::PathBuf::from(spec);
    let abs = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(&candidate)
    };

    // Normalize the path (resolve .. and . components) without requiring
    // the path to exist on disk (unlike canonicalize).
    let normalized = normalize_path(&abs);

    if let Ok(rel) = normalized.strip_prefix(work_tree) {
        rel.to_string_lossy().into_owned()
    } else {
        spec.to_owned()
    }
}

/// Normalize a path by resolving `.` and `..` components lexically.
fn normalize_path(path: &Path) -> std::path::PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

/// Write reflog entries for a checkout operation.
/// Resolve the previous branch from the HEAD reflog.
/// Looks for the most recent "checkout: moving from X to Y" entry and returns X.
/// Resolve `@{-N}` syntax to a branch name, returning the original string if not applicable.
fn resolve_at_minus(repo: &Repository, spec: &str) -> Result<String> {
    if spec.starts_with("@{-") && spec.ends_with('}') {
        if let Ok(n) = spec[3..spec.len() - 1].parse::<usize>() {
            return resolve_nth_previous_branch(repo, n);
        }
    }
    Ok(spec.to_string())
}

fn resolve_previous_branch(repo: &Repository) -> Result<String> {
    resolve_nth_previous_branch(repo, 1)
}

/// Resolve the Nth previously checked out branch from the HEAD reflog.
fn resolve_nth_previous_branch(repo: &Repository, n: usize) -> Result<String> {
    let reflog_path = repo.git_dir.join("logs/HEAD");
    let content = std::fs::read_to_string(&reflog_path).context("cannot read HEAD reflog")?;
    let mut seen = Vec::new();
    for line in content.lines().rev() {
        if let Some(msg_start) = line.find("checkout: moving from ") {
            let rest = &line[msg_start + "checkout: moving from ".len()..];
            if let Some(to_idx) = rest.find(" to ") {
                let from = &rest[..to_idx];
                // Only add if not already the most recently seen
                if seen.last().is_none_or(|last: &String| last != from) {
                    seen.push(from.to_string());
                }
                if seen.len() >= n {
                    return Ok(seen[n - 1].clone());
                }
            }
        }
    }
    bail!("no previous branch found in reflog")
}

fn write_checkout_reflog(
    repo: &Repository,
    head: &HeadState,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    message: &str,
) {
    let identity = resolve_checkout_identity(repo);

    // Write reflog for HEAD
    let _ = append_reflog(&repo.git_dir, "HEAD", old_oid, new_oid, &identity, message);

    // Write reflog for the branch ref if on a branch
    if let HeadState::Branch { refname, .. } = head {
        let _ = append_reflog(&repo.git_dir, refname, old_oid, new_oid, &identity, message);
    }
}

/// Resolve the committer identity for reflog entries.
fn resolve_checkout_identity(repo: &Repository) -> String {
    let config = ConfigSet::load(Some(&repo.git_dir), true).ok();
    let name = std::env::var("GIT_COMMITTER_NAME")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_NAME").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.name")))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
        .or_else(|| std::env::var("GIT_AUTHOR_EMAIL").ok())
        .or_else(|| config.as_ref().and_then(|c| c.get("user.email")))
        .unwrap_or_default();
    let now = time::OffsetDateTime::now_utc();
    let epoch = now.unix_timestamp();
    let offset = now.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{name} <{email}> {epoch} {hours:+03}{minutes:02}")
}
