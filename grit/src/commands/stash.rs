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
//! Subcommands: push, list, show, pop, apply, drop, clear.

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
        /// Also stash untracked files.
        #[arg(short = 'u', long = "include-untracked")]
        include_untracked: bool,
    },
    /// List stash entries.
    List,
    /// Show the diff of a stash entry.
    Show {
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Apply stash and remove it.
    Pop {
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Apply stash without removing it.
    Apply {
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Remove a stash entry.
    Drop {
        /// Stash reference (e.g. `stash@{0}`). Defaults to latest.
        stash: Option<String>,
    },
    /// Remove all stash entries.
    Clear,
}

/// Run `grit stash`.
pub fn run(args: Args) -> Result<()> {
    match args.command {
        None => {
            // Bare `grit stash` == `grit stash push`
            do_push(args.message, args.keep_index, args.include_untracked)
        }
        Some(StashCommand::Push {
            message,
            keep_index,
            include_untracked,
        }) => {
            // Merge: subcommand flags override top-level flags
            let msg = message.or(args.message);
            let ki = keep_index || args.keep_index;
            let iu = include_untracked || args.include_untracked;
            do_push(msg, ki, iu)
        }
        Some(StashCommand::List) => do_list(),
        Some(StashCommand::Show { stash }) => do_show(stash),
        Some(StashCommand::Pop { stash }) => do_pop(stash),
        Some(StashCommand::Apply { stash }) => do_apply(stash, false),
        Some(StashCommand::Drop { stash }) => do_drop(stash),
        Some(StashCommand::Clear) => do_clear(),
    }
}

// ---------------------------------------------------------------------------
// Push (save)
// ---------------------------------------------------------------------------

fn do_push(message: Option<String>, keep_index: bool, include_untracked: bool) -> Result<()> {
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

    // Find untracked files if requested
    let untracked_files = if include_untracked {
        find_untracked_files(&work_tree, &index)?
    } else {
        Vec::new()
    };

    if staged.is_empty() && unstaged.is_empty() && untracked_files.is_empty() {
        eprintln!("No local changes to save");
        return Ok(());
    }

    let now = OffsetDateTime::now_utc();
    let identity = resolve_identity(&repo, now)?;

    // 1. Create index-state commit (tree from current index)
    let index_tree_oid = write_tree_from_index(&repo.odb, &index, "")?;
    let index_commit_data = CommitData {
        tree: index_tree_oid,
        parents: vec![*head_oid],
        author: identity.clone(),
        committer: identity.clone(),
        encoding: None,
        message: "index on ".to_owned() + &branch_description(&head),
    };
    let index_commit_bytes = serialize_commit(&index_commit_data);
    let index_commit_oid = repo.odb.write(ObjectKind::Commit, &index_commit_bytes)?;

    // 2. Optionally create untracked-files commit
    let untracked_commit_oid = if !untracked_files.is_empty() {
        let tree_oid = create_untracked_tree(&repo.odb, &work_tree, &untracked_files)?;
        let ut_commit = CommitData {
            tree: tree_oid,
            parents: vec![*head_oid],
            author: identity.clone(),
            committer: identity.clone(),
            encoding: None,
            message: "untracked files on ".to_owned() + &branch_description(&head),
        };
        let ut_bytes = serialize_commit(&ut_commit);
        Some(repo.odb.write(ObjectKind::Commit, &ut_bytes)?)
    } else {
        None
    };

    // 3. Create the working-tree state commit (merges everything)
    //    First, build a tree that represents the complete working tree state
    //    for tracked files (index + worktree modifications)
    let wt_tree_oid = create_worktree_tree(&repo.odb, &index, &work_tree)?;

    let stash_msg = message.unwrap_or_else(|| {
        format!("WIP on {}", branch_description(&head))
    });

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
        message: stash_msg.clone(),
    };
    let stash_bytes = serialize_commit(&stash_commit);
    let stash_oid = repo.odb.write(ObjectKind::Commit, &stash_bytes)?;

    // 4. Update refs/stash and its reflog
    let old_stash = resolve_ref(&repo.git_dir, "refs/stash").ok();
    let zero_oid = ObjectId::from_hex("0000000000000000000000000000000000000000")?;
    let old_oid = old_stash.unwrap_or(zero_oid);

    write_ref(&repo.git_dir, "refs/stash", &stash_oid)?;
    grit_lib::refs::append_reflog(
        &repo.git_dir,
        "refs/stash",
        &old_oid,
        &stash_oid,
        &identity,
        &stash_msg,
    )?;

    // 5. Clean working tree: reset to HEAD state
    if keep_index {
        // Reset working tree to index state (keep staged changes)
        reset_worktree_to_index(&repo, &index, &work_tree)?;
    } else {
        // Reset index and working tree to HEAD
        reset_to_head(&repo, head_oid, &work_tree)?;
    }

    // Remove untracked files if they were stashed
    if include_untracked {
        for f in &untracked_files {
            let path = work_tree.join(f);
            let _ = fs::remove_file(&path);
            // Clean up empty parent dirs
            if let Some(parent) = path.parent() {
                remove_empty_dirs(parent, &work_tree);
            }
        }
    }

    eprintln!(
        "Saved working directory and index state {}",
        stash_msg
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

fn do_list() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let entries = read_reflog(&repo.git_dir, "refs/stash")?;
    // Entries are in file order (oldest first), display newest first
    for (i, entry) in entries.iter().rev().enumerate() {
        println!("stash@{{{i}}}: {}", entry.message);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Show
// ---------------------------------------------------------------------------

fn do_show(stash_ref: Option<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stash_oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    let obj = repo.odb.read(&stash_oid)?;
    let stash_commit = parse_commit(&obj.data)?;

    // The stash commit's tree vs its first parent (HEAD at stash time)
    let parent_oid = stash_commit
        .parents
        .first()
        .ok_or_else(|| anyhow::anyhow!("corrupt stash commit: no parents"))?;
    let parent_obj = repo.odb.read(parent_oid)?;
    let parent_commit = parse_commit(&parent_obj.data)?;

    // Flatten both trees and show diff
    let old_entries = flatten_tree_full(&repo.odb, &parent_commit.tree, "")?;
    let new_entries = flatten_tree_full(&repo.odb, &stash_commit.tree, "")?;

    show_tree_diff(&repo.odb, &old_entries, &new_entries)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Apply / Pop
// ---------------------------------------------------------------------------

fn do_pop(stash_ref: Option<String>) -> Result<()> {
    do_apply(stash_ref.clone(), true)
}

fn do_apply(stash_ref: Option<String>, drop_after: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot apply stash in a bare repository"))?
        .to_path_buf();

    let stash_index = parse_stash_index(stash_ref.as_deref())?;
    let stash_oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    let obj = repo.odb.read(&stash_oid)?;
    let stash_commit = parse_commit(&obj.data)?;

    if stash_commit.parents.len() < 2 {
        bail!("corrupt stash commit: expected at least 2 parents");
    }

    let _head_at_stash = &stash_commit.parents[0];
    let index_commit_oid = &stash_commit.parents[1];

    // Read the index commit to get the staged state
    let index_obj = repo.odb.read(index_commit_oid)?;
    let index_commit = parse_commit(&index_obj.data)?;

    // Apply: restore the working tree from the stash commit's tree
    let stash_tree_entries = flatten_tree_full(&repo.odb, &stash_commit.tree, "")?;
    let index_tree_entries = flatten_tree_full(&repo.odb, &index_commit.tree, "")?;

    // Restore the index to the stashed index state
    let new_index = build_index_from_tree(&repo.odb, &index_tree_entries)?;

    // Restore working tree files from the stash commit's tree
    for entry in &stash_tree_entries {
        let file_path = work_tree.join(&entry.path);
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

    // Write the restored index
    new_index.write(&repo.index_path())?;

    if drop_after {
        drop_stash_entry(&repo, stash_index)?;
        eprintln!("Dropped refs/stash@{{{stash_index}}}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Drop
// ---------------------------------------------------------------------------

fn do_drop(stash_ref: Option<String>) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let stash_index = parse_stash_index(stash_ref.as_deref())?;

    // Verify it exists
    let _oid = resolve_stash_ref(&repo, stash_ref.as_deref())?;

    drop_stash_entry(&repo, stash_index)?;
    eprintln!("Dropped refs/stash@{{{stash_index}}}");
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
        // Top of stash is last entry's new_oid
        let top = &remaining.last().ok_or_else(|| anyhow::anyhow!("stash entries unexpectedly empty"))?.new_oid;
        write_ref(&repo.git_dir, "refs/stash", top)?;
    }

    Ok(())
}

/// Get a branch description string for stash messages.
fn branch_description(head: &HeadState) -> String {
    match head {
        HeadState::Branch { refname, oid, .. } => {
            let name = refname
                .strip_prefix("refs/heads/")
                .unwrap_or(refname);
            match oid {
                Some(oid) => format!("{name}: {}", &oid.to_hex()[..7]),
                None => name.to_string(),
            }
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
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var("GIT_COMMITTER_EMAIL")
        .ok()
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
            // Subtree
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

    // All paths
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
                    // Show content diff
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
    // Build a nested tree structure
    use std::collections::BTreeMap;

    // Group files by top-level directory
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
                entries.push(TreeEntry { mode, name: name.into_bytes(), oid });
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
    let oid = builder.write(odb)?;
    Ok(oid)
}

/// Create a tree representing the working tree state of all tracked files.
fn create_worktree_tree(
    odb: &Odb,
    index: &Index,
    work_tree: &Path,
) -> Result<ObjectId> {
    // Start from index, overlay worktree changes
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
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File deleted — it won't be in the worktree tree
                // Mark for removal by setting a sentinel
                entry.oid = ObjectId::from_hex("0000000000000000000000000000000000000000")?;
            }
            Err(e) => return Err(e.into()),
        }
    }

    // Remove entries with zero OID (deleted files)
    let zero = ObjectId::from_hex("0000000000000000000000000000000000000000")?;
    temp_index.entries.retain(|e| e.oid != zero);

    write_tree_from_index(odb, &temp_index, "").map_err(Into::into)
}

/// Build an Index from a flattened tree.
fn build_index_from_tree(_odb: &Odb, entries: &[FlatTreeEntry]) -> Result<Index> {
    let mut index = Index::new();
    for entry in entries {
        let path_len = entry.path.len();
        let flags = if path_len > 0xFFF { 0xFFF } else { path_len as u16 };
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

    // Build new index from HEAD tree
    let tree_entries = flatten_tree_full(&repo.odb, &head_commit.tree, "")?;
    let new_index = build_index_from_tree(&repo.odb, &tree_entries)?;

    // Restore working tree files
    for entry in &tree_entries {
        let file_path = work_tree.join(&entry.path);
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

    // Write the new index
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

/// Remove empty parent directories up to (but not including) the work tree root.
fn remove_empty_dirs(dir: &Path, stop_at: &Path) {
    let mut current = dir.to_path_buf();
    while current != stop_at {
        if fs::read_dir(&current).map(|mut d| d.next().is_none()).unwrap_or(false) {
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
