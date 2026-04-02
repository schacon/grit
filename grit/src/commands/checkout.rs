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

use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs;
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

    /// Force: discard local changes.
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Remaining positional arguments: `[<branch|commit>] [--] [<paths>...]`
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub rest: Vec<String>,
}

/// Run `grit checkout`.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;

    // Detect if `--` was used in the original command line. Clap strips a
    // leading `--` from trailing_var_arg, so we check the raw args.
    let raw_args: Vec<String> = std::env::args().collect();
    let has_separator = raw_args.iter().any(|a| a == "--");

    // Parse rest into (target, paths) handling `--` separator
    let (target, paths) = split_target_and_paths(&args.rest, has_separator);

    // Case 1: checkout -b <new_branch> [<start_point>]
    if let Some(ref new_branch_name) = args.new_branch {
        return create_and_switch_branch(&repo, new_branch_name, target.as_deref(), args.force);
    }

    // Case 2: checkout [<tree-ish>] -- <paths>  (path restore)
    if !paths.is_empty() {
        return checkout_paths(&repo, target.as_deref(), &paths);
    }

    // Case 3: checkout -- (with no paths and no target) is a no-op
    // Case 4: checkout <branch-or-commit>
    let target = match target {
        Some(t) => t,
        None => bail!("you must specify a branch, commit, or paths to checkout"),
    };

    // Try as a branch first
    let branch_ref = format!("refs/heads/{target}");
    if refs::resolve_ref(&repo.git_dir, &branch_ref).is_ok() {
        return switch_branch(&repo, &target, &branch_ref, args.force);
    }

    // Try as a commit (detached HEAD)
    match resolve_to_commit(&repo, &target) {
        Ok(oid) => detach_head(&repo, &oid, args.force),
        Err(_) => bail!("pathspec '{}' did not match any file(s) known to git", target),
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
fn switch_branch(repo: &Repository, branch_name: &str, branch_ref: &str, force: bool) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    // Check if already on this branch
    if let HeadState::Branch { ref refname, .. } = head {
        if refname == branch_ref {
            eprintln!("Already on '{}'", branch_name);
            return Ok(());
        }
    }

    let target_oid = refs::resolve_ref(&repo.git_dir, branch_ref)
        .with_context(|| format!("cannot resolve branch '{branch_name}'"))?;

    let target_tree = commit_to_tree(repo, &target_oid)?;

    // Update working tree and index
    switch_to_tree(repo, &head, &target_tree, force)?;

    // Update HEAD to point to the branch
    std::fs::write(
        repo.git_dir.join("HEAD"),
        format!("ref: {branch_ref}\n"),
    )?;

    println!("Switched to branch '{}'", branch_name);
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
    let start_oid = match start {
        Some(s) => resolve_to_commit(repo, s)?,
        None => {
            let head = resolve_head(&repo.git_dir)?;
            match head.oid() {
                Some(oid) => *oid,
                None => bail!("cannot create branch '{}': HEAD does not point to a commit", name),
            }
        }
    };

    let head = resolve_head(&repo.git_dir)?;
    let target_tree = commit_to_tree(repo, &start_oid)?;

    // Update working tree if start point differs from current HEAD
    if head.oid() != Some(&start_oid) {
        switch_to_tree(repo, &head, &target_tree, force)?;
    }

    // Create the branch ref
    refs::write_ref(&repo.git_dir, &branch_ref, &start_oid)?;

    // Update HEAD to point to the new branch
    std::fs::write(
        repo.git_dir.join("HEAD"),
        format!("ref: {branch_ref}\n"),
    )?;

    println!("Switched to a new branch '{}'", name);
    Ok(())
}

/// Detach HEAD at a specific commit.
fn detach_head(repo: &Repository, oid: &ObjectId, force: bool) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;
    let target_tree = commit_to_tree(repo, oid)?;

    // Update working tree
    switch_to_tree(repo, &head, &target_tree, force)?;

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
        check_dirty_worktree(repo, &old_index, &new_index, &work_tree)?;
    }

    // Perform the actual working tree update
    checkout_index_to_worktree(repo, &old_index, &new_index, &work_tree)?;

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
        let mut msg = String::from("error: Your local changes to the following files would be overwritten by checkout:\n");
        for path in &would_overwrite {
            msg.push_str(&format!("\t{}\n", path));
        }
        msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
        bail!("{}", msg);
    }

    Ok(())
}

/// Check if a working tree file differs from its index entry.
fn is_worktree_dirty(repo: &Repository, entry: &IndexEntry, abs_path: &std::path::Path) -> Result<bool> {
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
fn checkout_paths(repo: &Repository, source: Option<&str>, paths: &[String]) -> Result<()> {
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

                let entry = index.get(path_bytes, 0)
                    .ok_or_else(|| anyhow::anyhow!(
                        "error: pathspec '{}' did not match any file(s) known to git",
                        path_str
                    ))?;

                let obj = repo.odb.read(&entry.oid)
                    .with_context(|| format!("reading blob for '{rel}'"))?;
                if obj.kind != ObjectKind::Blob {
                    bail!("'{}' is not a blob in the index", rel);
                }
                write_to_worktree(work_tree, &rel, &obj.data, entry.mode)?;
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

                let (blob_oid, mode) = find_in_tree(repo, tree_oid, &rel)?
                    .ok_or_else(|| anyhow::anyhow!(
                        "error: pathspec '{}' did not match any file(s) known to git",
                        path_str
                    ))?;

                let obj = repo.odb.read(&blob_oid)
                    .with_context(|| format!("reading blob for '{rel}'"))?;
                if obj.kind != ObjectKind::Blob {
                    bail!("'{}' is not a blob in the source tree", rel);
                }

                // Write to working tree
                write_to_worktree(work_tree, &rel, &obj.data, mode)?;

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

            if index_modified {
                index.write(&index_path).context("writing index")?;
            }
        }
    }

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
    println!("HEAD is now at {} {}", abbrev, subject);
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

        // Skip unchanged entries (same OID and mode)
        if let Some(old_entry) = old_map.get(entry.path.as_slice()) {
            if old_entry.oid == entry.oid && old_entry.mode == entry.mode {
                continue;
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
    let obj = repo
        .odb
        .read(oid)
        .context("reading object for checkout")?;
    if obj.kind != ObjectKind::Blob {
        bail!("cannot checkout non-blob at '{rel_path}'");
    }

    write_to_worktree(work_tree, rel_path, &obj.data, mode)
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

/// Resolve a pathspec to a repository-relative path.
fn resolve_pathspec(spec: &str, work_tree: &Path, cwd: &Path) -> String {
    let candidate = std::path::PathBuf::from(spec);
    let abs = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(&candidate)
    };

    if let Ok(rel) = abs.strip_prefix(work_tree) {
        rel.to_string_lossy().into_owned()
    } else {
        spec.to_owned()
    }
}
