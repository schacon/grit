//! `grit reset` — reset current HEAD to the specified state.
//!
//! Implements the following modes:
//!
//! - `--soft`  : move HEAD only; index and working tree unchanged.
//! - `--mixed` : move HEAD and reset index to the target tree (default).
//! - `--hard`  : move HEAD, reset index, and update working tree.
//! - `--keep`  : like --hard but refuse if uncommitted local changes would be lost.
//!
//! When path arguments are given the HEAD is not moved; only the index entries
//! for those paths are reset to the content of the target commit's tree.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::odb::Odb;
use grit_lib::refs::{append_reflog, write_ref};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::{abbreviate_object_id, resolve_revision};
use grit_lib::state::{resolve_head, HeadState};

/// The zero OID for reflog entries when there is no previous value.
fn zero_oid() -> ObjectId {
    ObjectId::from_hex("0000000000000000000000000000000000000000").expect("zero oid is valid")
}

/// The reset mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ResetMode {
    Soft,
    #[default]
    Mixed,
    Hard,
    Keep,
    Merge,
}

impl ResetMode {
    fn name(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Mixed => "mixed",
            Self::Hard => "hard",
            Self::Keep => "keep",
            Self::Merge => "merge",
        }
    }
}

/// Arguments for `grit reset`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Move HEAD only; do not touch the index or working tree.
    #[arg(long)]
    pub soft: bool,

    /// Reset index to the target tree but leave working tree unchanged (default).
    #[arg(long)]
    pub mixed: bool,

    /// Reset index and working tree to the target tree.
    #[arg(long)]
    pub hard: bool,

    /// Like --hard but refuse to reset if uncommitted changes would be lost.
    #[arg(long)]
    pub keep: bool,

    /// Reset index and working tree like --hard, but keep local changes where possible.
    #[arg(long = "merge")]
    pub merge: bool,

    /// Suppress feedback messages.
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Record the fact that removed paths will be re-added later (intent-to-add).
    #[arg(short = 'N', long = "intent-to-add")]
    pub intent_to_add: bool,

    /// Do not refresh the index after a mixed reset.
    #[arg(long = "no-refresh")]
    pub no_refresh: bool,

    /// Refresh the index after a mixed reset (default).
    #[arg(long = "refresh")]
    pub refresh: bool,

    /// Interactive patch mode.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Remaining positional arguments: `[<commit>] [--] [<path>…]`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub rest: Vec<String>,
}

/// Pre-validate raw arguments before clap parsing, catching Git-specific
/// negated flags that clap doesn't know about.
pub fn pre_validate_args(raw_args: &[String]) -> Result<()> {
    for arg in raw_args {
        // Check for negated reset mode flags
        for mode in &["soft", "mixed", "hard", "merge", "keep"] {
            if arg == &format!("--no-{mode}") {
                bail!("unknown option `no-{mode}'");
            }
        }
    }
    Ok(())
}

/// Filter out `--end-of-options` from args (replace with `--`).
pub fn filter_args(raw_args: &[String]) -> Vec<String> {
    raw_args
        .iter()
        .map(|a| {
            if a == "--end-of-options" {
                "--".to_owned()
            } else {
                a.clone()
            }
        })
        .collect()
}

/// Run `grit reset`.
pub fn run(args: Args) -> Result<()> {
    let mode = parse_mode(&args)?;

    let repo = Repository::discover(None).context("not a git repository")?;

    // Handle -p (patch mode) by delegating to `git checkout-index`-like interactive unstaging
    if args.patch {
        return reset_patch(&repo, &args.rest);
    }

    // Split positional args into (commit_spec, paths).
    let (commit_spec, paths) = split_commit_and_paths(&repo, &args.rest);

    if !paths.is_empty() {
        // Pathspec reset: only update index entries, HEAD stays put.
        if mode != ResetMode::Mixed {
            bail!("Cannot do --{} reset with paths.", mode.name());
        }
        return reset_paths(&repo, &commit_spec, &paths, args.quiet, args.intent_to_add);
    }

    reset_commit(&repo, &commit_spec, mode, args.quiet)
}

/// Parse the reset mode from the flag combination.
fn parse_mode(args: &Args) -> Result<ResetMode> {
    match (args.soft, args.mixed, args.hard, args.keep, args.merge) {
        (true, false, false, false, false) => Ok(ResetMode::Soft),
        (false, true, false, false, false) => Ok(ResetMode::Mixed),
        (false, false, true, false, false) => Ok(ResetMode::Hard),
        (false, false, false, true, false) => Ok(ResetMode::Keep),
        (false, false, false, false, true) => Ok(ResetMode::Merge),
        (false, false, false, false, false) => Ok(ResetMode::default()),
        _ => bail!("cannot mix --soft, --mixed, --hard, --keep, and --merge"),
    }
}

/// Split positional arguments into `(commit_spec, paths)`.
///
/// Handles the `--` end-of-options separator explicitly (clap passes it
/// through when `trailing_var_arg` is in use).  If the first argument
/// resolves as a commit-ish it is used as the commit spec and the rest are
/// paths; otherwise `"HEAD"` is assumed and all arguments are paths.
fn split_commit_and_paths(repo: &Repository, rest: &[String]) -> (String, Vec<String>) {
    if rest.is_empty() {
        return ("HEAD".to_owned(), vec![]);
    }

    // Detect an explicit `--` or `--end-of-options` separator.
    if let Some(sep) = rest
        .iter()
        .position(|a| a == "--" || a == "--end-of-options")
    {
        // Everything before `--` is the optional commit; everything after is paths.
        let commit_spec = if sep == 0 {
            "HEAD".to_owned()
        } else {
            rest[0].clone()
        };
        let paths = rest[sep + 1..].to_vec();
        return (commit_spec, paths);
    }

    let first = &rest[0];
    // Attempt to resolve first arg as a commit-ish.
    // Must actually resolve to a commit (not just any object like a blob).
    let first_is_commit = resolve_revision(repo, first)
        .ok()
        .and_then(|oid| peel_to_commit(repo, oid).ok())
        .is_some();

    if first_is_commit {
        // First arg is the commit; remaining args are paths (may be empty).
        (first.clone(), rest[1..].to_vec())
    } else {
        // First arg is not a commit: treat all args as paths against HEAD.
        ("HEAD".to_owned(), rest.to_vec())
    }
}

/// Resolve the committer identity for reflog entries.
fn resolve_reflog_identity(repo: &Repository) -> String {
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

/// Write reflog entries for a reset operation.
fn write_reset_reflog(
    repo: &Repository,
    head: &HeadState,
    old_oid: &ObjectId,
    new_oid: &ObjectId,
    commit_spec: &str,
) {
    let identity = resolve_reflog_identity(repo);
    let message = format!("reset: moving to {commit_spec}");

    // Write reflog for HEAD.
    let _ = append_reflog(&repo.git_dir, "HEAD", old_oid, new_oid, &identity, &message);

    // Write reflog for the branch ref if on a branch.
    if let HeadState::Branch { refname, .. } = head {
        let _ = append_reflog(
            &repo.git_dir,
            refname,
            old_oid,
            new_oid,
            &identity,
            &message,
        );
    }
}

/// Interactive patch-mode reset: present each staged hunk and ask whether
/// to unstage it. This is the `git reset -p` flow.
fn reset_patch(repo: &Repository, _rest: &[String]) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let head = resolve_head(&repo.git_dir)?;
    let index_path = repo.index_path();
    let mut index = Index::load(&index_path).context("loading index")?;

    // Get HEAD tree entries (empty if unborn)
    let tree_entries = if let Some(oid) = head.oid() {
        let tree_oid = commit_to_tree(repo, oid)?;
        tree_to_flat_entries(repo, &tree_oid, "")?
    } else {
        Vec::new()
    };
    let tree_map: HashMap<Vec<u8>, IndexEntry> = tree_entries
        .into_iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    // Find staged entries that differ from HEAD
    let mut staged_paths: Vec<Vec<u8>> = Vec::new();
    for entry in &index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let in_tree = tree_map.get(&entry.path);
        let differs = match in_tree {
            Some(te) => te.oid != entry.oid || te.mode != entry.mode,
            None => true, // new file
        };
        if differs {
            staged_paths.push(entry.path.clone());
        }
    }
    // Also find paths deleted from index (in tree but not in index)
    for path in tree_map.keys() {
        if !index
            .entries
            .iter()
            .any(|e| e.path == *path && e.stage() == 0)
        {
            if !staged_paths.contains(path) {
                staged_paths.push(path.clone());
            }
        }
    }

    if staged_paths.is_empty() {
        return Ok(());
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();

    for path in &staged_paths {
        let path_str = String::from_utf8_lossy(path);
        let action = if tree_map.contains_key(path) {
            "Unstage"
        } else {
            "Unstage addition of"
        };
        print!("{}  {}? ([y]es/[n]o) ", action, path_str);
        io::stdout().flush()?;
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let answer = line.trim().to_lowercase();
        if answer == "y" || answer == "yes" {
            // Reset this path to tree version
            index.remove(path);
            if let Some(te) = tree_map.get(path) {
                index.add_or_replace(te.clone());
            }
        }
    }

    index.write(&index_path).context("writing index")?;
    Ok(())
}

/// Reset specific index entries to match the given commit's tree.
///
/// HEAD is not modified.
fn reset_paths(
    repo: &Repository,
    commit_spec: &str,
    paths: &[String],
    _quiet: bool,
    intent_to_add: bool,
) -> Result<()> {
    // On an unborn branch, the tree is empty (no commit exists yet).
    let tree_entries = match resolve_to_commit(repo, commit_spec) {
        Ok(commit_oid) => {
            let tree_oid = commit_to_tree(repo, &commit_oid)?;
            tree_to_flat_entries(repo, &tree_oid, "")?
        }
        Err(_) => {
            // Check if HEAD is unborn
            let head = resolve_head(&repo.git_dir)?;
            if head.oid().is_none() && commit_spec == "HEAD" {
                Vec::new() // empty tree
            } else {
                bail!(
                    "unknown revision: '{}': object not found: {}",
                    commit_spec,
                    commit_spec
                );
            }
        }
    };

    // Build a lookup table: path bytes → IndexEntry.
    let mut tree_map: HashMap<Vec<u8>, IndexEntry> = HashMap::new();
    for e in tree_entries {
        tree_map.insert(e.path.clone(), e);
    }

    let index_path = repo.index_path();
    let mut index = Index::load(&index_path).context("loading index")?;

    for path_str in paths {
        let path_bytes = path_str.as_bytes().to_vec();

        // A path must exist in the target tree or in the current index.
        let in_tree = tree_map.contains_key(&path_bytes);
        let in_index = index.entries.iter().any(|e| e.path == path_bytes);
        if !in_tree && !in_index {
            bail!("pathspec '{path_str}' did not match any file(s) known to git");
        }

        // Remove all stages for this path.
        index.remove(&path_bytes);
        // Re-add from tree if present.
        if let Some(entry) = tree_map.get(&path_bytes) {
            index.add_or_replace(entry.clone());
        } else if intent_to_add {
            // With -N, keep removed paths as intent-to-add entries.
            let empty_oid = ObjectId::from_hex("e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")
                .expect("empty blob oid is valid");
            let mut ita_entry = IndexEntry {
                ctime_sec: 0,
                ctime_nsec: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                size: 0,
                oid: empty_oid,
                flags: path_bytes.len().min(0xFFF) as u16,
                flags_extended: None,
                path: path_bytes.clone(),
            };
            ita_entry.set_intent_to_add(true);
            index.add_or_replace(ita_entry);
        }
        // If not in tree and no -N, path is removed from index (staged deletion).
    }

    index.write(&index_path).context("writing index")?;
    Ok(())
}

/// Reset HEAD (and optionally index + working tree) to the given commit.
fn reset_commit(repo: &Repository, commit_spec: &str, mode: ResetMode, quiet: bool) -> Result<()> {
    let head = resolve_head(&repo.git_dir)?;

    // --soft fails when there are unmerged entries or a merge is in progress.
    if mode == ResetMode::Soft {
        if repo.git_dir.join("MERGE_HEAD").exists() {
            bail!("Cannot do a soft reset in the middle of a merge.");
        }
        if repo.git_dir.join("CHERRY_PICK_HEAD").exists() {
            bail!("Cannot do a soft reset in the middle of a cherry-pick.");
        }
        if repo.git_dir.join("REVERT_HEAD").exists() {
            bail!("Cannot do a soft reset in the middle of a revert.");
        }
        let index_path = repo.index_path();
        let index = Index::load(&index_path).context("loading index")?;
        if index.entries.iter().any(|e| e.stage() != 0) {
            bail!("Cannot do a soft reset in the middle of a merge.");
        }
    }

    let target_oid = match resolve_to_commit(repo, commit_spec) {
        Ok(oid) => oid,
        Err(_) if head.oid().is_none() => {
            // Unborn branch handling
            match mode {
                ResetMode::Soft => {
                    // --soft on unborn: no-op (nothing to move)
                    return Ok(());
                }
                ResetMode::Mixed => {
                    // Mixed on unborn: clear the index
                    let index_path = repo.index_path();
                    let new_index = Index::new();
                    new_index.write(&index_path).context("writing index")?;
                    return Ok(());
                }
                ResetMode::Hard | ResetMode::Merge => {
                    // Unborn branch: reset --hard just clears the index and working tree
                    let index_path = repo.index_path();
                    let old_index = match Index::load(&index_path) {
                        Ok(idx) => idx,
                        Err(_) => Index::new(),
                    };
                    let new_index = Index::new();
                    if let Some(_wt) = &repo.work_tree {
                        checkout_index_to_worktree(repo, &old_index, &mut new_index.clone())?;
                    }
                    new_index.write(&index_path).context("writing index")?;
                    return Ok(());
                }
                ResetMode::Keep => {
                    return Ok(());
                }
            }
        }
        Err(e) => return Err(e),
    };

    // For --keep, we need to check safety before making any changes.
    if mode == ResetMode::Keep {
        check_keep_safety(repo, &head, &target_oid)?;
    }

    // Get the old OID for reflog and ORIG_HEAD.
    let old_oid = head.oid().copied().unwrap_or_else(zero_oid);

    // Save ORIG_HEAD before moving HEAD.
    if head.oid().is_some() {
        write_orig_head(&repo.git_dir, &old_oid)?;
    }

    // Update HEAD (and the branch it points to, if on a branch).
    update_head_ref(&repo.git_dir, &head, &target_oid)?;

    // Write reflog entries.
    write_reset_reflog(repo, &head, &old_oid, &target_oid, commit_spec);

    // Clean up merge/cherry-pick state files on non-soft reset.
    if mode != ResetMode::Soft {
        let _ = std::fs::remove_file(repo.git_dir.join("MERGE_HEAD"));
        let _ = std::fs::remove_file(repo.git_dir.join("MERGE_MSG"));
        let _ = std::fs::remove_file(repo.git_dir.join("MERGE_MODE"));
        let _ = std::fs::remove_file(repo.git_dir.join("CHERRY_PICK_HEAD"));
        let _ = std::fs::remove_file(repo.git_dir.join("REVERT_HEAD"));
    }

    if mode == ResetMode::Soft {
        // Soft: HEAD moved, index and working tree unchanged.
        return Ok(());
    }

    // Mixed / hard / keep: reset index from the commit's tree.
    let tree_oid = commit_to_tree(repo, &target_oid)?;
    let tree_entries = tree_to_flat_entries(repo, &tree_oid, "")?;

    let index_path = repo.index_path();
    let old_index = Index::load(&index_path).context("loading old index")?;
    let mut new_index = Index::new();
    new_index.entries = tree_entries;
    new_index.sort();

    if mode == ResetMode::Hard || mode == ResetMode::Keep || mode == ResetMode::Merge {
        // Hard/Keep: also update working tree.
        if repo.work_tree.is_some() {
            checkout_index_to_worktree(repo, &old_index, &mut new_index)?;
        }
        if !quiet {
            print_head_message(repo, &target_oid)?;
        }
    } else if mode == ResetMode::Mixed && !quiet {
        // Mixed: print unstaged changes after reset.
        // We need to print before writing the new index.
        print_unstaged_changes(repo, &new_index)?;
    }

    new_index.write(&index_path).context("writing index")?;
    Ok(())
}

/// Check if `--keep` is safe: refuse if there are local uncommitted changes
/// to files that differ between HEAD and the target.
fn check_keep_safety(repo: &Repository, head: &HeadState, target_oid: &ObjectId) -> Result<()> {
    let head_oid = match head.oid() {
        Some(oid) => *oid,
        None => return Ok(()), // unborn branch, nothing to protect
    };

    if head_oid == *target_oid {
        return Ok(()); // no-op reset
    }

    // Get trees for HEAD and target.
    let head_tree_oid = commit_to_tree(repo, &head_oid)?;
    let target_tree_oid = commit_to_tree(repo, target_oid)?;

    let head_entries = tree_to_flat_entries(repo, &head_tree_oid, "")?;
    let target_entries = tree_to_flat_entries(repo, &target_tree_oid, "")?;

    let head_map: HashMap<Vec<u8>, &IndexEntry> =
        head_entries.iter().map(|e| (e.path.clone(), e)).collect();
    let target_map: HashMap<Vec<u8>, &IndexEntry> =
        target_entries.iter().map(|e| (e.path.clone(), e)).collect();

    // Files that differ between HEAD and target.
    let mut changed_paths: HashSet<Vec<u8>> = HashSet::new();

    // Files in HEAD but not in target (or different).
    for (path, head_entry) in &head_map {
        match target_map.get(path) {
            Some(target_entry)
                if target_entry.oid == head_entry.oid && target_entry.mode == head_entry.mode => {}
            _ => {
                changed_paths.insert(path.clone());
            }
        }
    }
    // Files in target but not in HEAD.
    for path in target_map.keys() {
        if !head_map.contains_key(path) {
            changed_paths.insert(path.clone());
        }
    }

    if changed_paths.is_empty() {
        return Ok(());
    }

    // Check if any of these changed paths have local modifications in the
    // working tree or index that differ from HEAD.
    let index_path = repo.index_path();
    let index = Index::load(&index_path).context("loading index")?;
    let index_map: HashMap<Vec<u8>, &IndexEntry> = index
        .entries
        .iter()
        .filter(|e| e.stage() == 0)
        .map(|e| (e.path.clone(), e))
        .collect();

    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    for path in &changed_paths {
        let path_str = String::from_utf8_lossy(path);

        // Check index vs HEAD.
        let head_entry = head_map.get(path);
        let idx_entry = index_map.get(path);

        match (head_entry, idx_entry) {
            (Some(h), Some(i)) => {
                if h.oid != i.oid || h.mode != i.mode {
                    bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
                }
            }
            (None, Some(_)) => {
                // File is in index but not in HEAD — local addition.
                bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
            }
            (Some(_), None) => {
                // File was in HEAD but not in index — staged deletion.
                bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
            }
            (None, None) => {}
        }

        // Check working tree vs index.
        let abs_path = work_tree.join(&*path_str);
        if let Some(idx_e) = idx_entry {
            if abs_path.exists() {
                // Compare file content with index entry.
                if let Ok(content) = std::fs::read(&abs_path) {
                    let worktree_oid = hash_blob_content(&content);
                    if worktree_oid != idx_e.oid {
                        bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
                    }
                }
            } else {
                // File in index but deleted in worktree.
                bail!("Entry '{}' not uptodate. Cannot merge.", path_str);
            }
        } else if abs_path.exists() {
            // Untracked file would be overwritten.
            let target_entry = target_map.get(path);
            if target_entry.is_some() {
                bail!(
                    "Entry '{}' would be overwritten by merge. Cannot merge.",
                    path_str
                );
            }
        }
    }

    Ok(())
}

/// Compute the git blob OID for raw content (without writing to ODB).
fn hash_blob_content(data: &[u8]) -> ObjectId {
    Odb::hash_object_data(ObjectKind::Blob, data)
}

/// Print "Unstaged changes after reset:" with modified files (mixed mode).
///
/// Compares the new index against the working tree. Files that differ are
/// printed as `M\t<path>`.
fn print_unstaged_changes(repo: &Repository, new_index: &Index) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    let mut modified: Vec<String> = Vec::new();

    for entry in &new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if !abs_path.exists() {
            // File in index but not in worktree — deleted, counts as modified.
            modified.push(path_str);
            continue;
        }

        // Compare content.
        if entry.mode == MODE_SYMLINK {
            if let Ok(target) = std::fs::read_link(&abs_path) {
                let target_str = target.to_string_lossy();
                let obj = repo.odb.read(&entry.oid);
                if let Ok(obj) = obj {
                    let index_target = String::from_utf8_lossy(&obj.data);
                    if target_str != index_target.as_ref() {
                        modified.push(path_str);
                    }
                }
            } else {
                modified.push(path_str);
            }
        } else if let Ok(content) = std::fs::read(&abs_path) {
            let worktree_oid = hash_blob_content(&content);
            if worktree_oid != entry.oid {
                modified.push(path_str);
            }
        } else {
            modified.push(path_str);
        }
    }

    if !modified.is_empty() {
        println!("Unstaged changes after reset:");
        for path in &modified {
            println!("M\t{path}");
        }
    }

    Ok(())
}

/// Write `.git/ORIG_HEAD`.
fn write_orig_head(git_dir: &Path, oid: &ObjectId) -> Result<()> {
    std::fs::write(git_dir.join("ORIG_HEAD"), format!("{oid}\n"))?;
    Ok(())
}

/// Update HEAD and the branch ref it resolves to.
fn update_head_ref(git_dir: &Path, head: &HeadState, new_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            write_ref(git_dir, refname, new_oid)?;
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            std::fs::write(git_dir.join("HEAD"), format!("{new_oid}\n"))?;
        }
    }
    Ok(())
}

/// Print `"HEAD is now at <abbrev> <subject>\n"` to stdout.
fn print_head_message(repo: &Repository, oid: &ObjectId) -> Result<()> {
    let obj = repo.odb.read(oid)?;
    if obj.kind != ObjectKind::Commit {
        return Ok(());
    }
    let commit = parse_commit(&obj.data)?;
    let subject = commit.message.lines().next().unwrap_or("").trim();
    let abbrev =
        abbreviate_object_id(repo, *oid, 7).unwrap_or_else(|_| oid.to_hex()[..7].to_owned());
    println!("HEAD is now at {abbrev} {subject}");
    Ok(())
}

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

/// Update the working tree to match the new index (used for `--hard`/`--keep` reset).
///
/// Deletes files removed from the index and writes/updates files added or
/// changed.
fn checkout_index_to_worktree(
    repo: &Repository,
    old_index: &Index,
    new_index: &mut Index,
) -> Result<()> {
    let work_tree = match &repo.work_tree {
        Some(p) => p.clone(),
        None => return Ok(()),
    };

    // Load gitattributes and config for CRLF conversion
    let attr_rules = grit_lib::crlf::load_gitattributes(&work_tree);
    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok();
    let conv = config
        .as_ref()
        .map(grit_lib::crlf::ConversionConfig::from_config);

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

    // Remove paths that are no longer present in the new index.
    for old_path in old_stage0.difference(&new_stage0) {
        let rel = String::from_utf8_lossy(old_path).into_owned();
        let abs = work_tree.join(&rel);
        if abs.is_file() || abs.is_symlink() {
            let _ = std::fs::remove_file(&abs);
        } else if abs.is_dir() {
            let _ = std::fs::remove_dir_all(&abs);
        }
        remove_empty_parent_dirs(&work_tree, &abs);
    }

    // Write all stage-0 entries from the new index.
    for entry in &mut new_index.entries {
        if entry.stage() != 0 {
            continue;
        }
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let obj = repo
            .odb
            .read(&entry.oid)
            .context("reading object for checkout")?;
        if obj.kind != ObjectKind::Blob {
            bail!("cannot checkout non-blob at '{path_str}'");
        }

        if abs_path.is_dir() {
            std::fs::remove_dir_all(&abs_path)?;
        }

        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink target is not UTF-8"))?;
            if abs_path.exists() || abs_path.is_symlink() {
                std::fs::remove_file(&abs_path)?;
            }
            std::os::unix::fs::symlink(target, &abs_path)?;
        } else {
            // Apply CRLF conversion if configured
            let data = if let (Some(ref cfg), Some(ref cv)) = (&config, &conv) {
                let file_attrs = grit_lib::crlf::get_file_attrs(&attr_rules, &path_str, cfg);
                grit_lib::crlf::convert_to_worktree(&obj.data, &path_str, cv, &file_attrs, None)
            } else {
                obj.data.clone()
            };
            std::fs::write(&abs_path, &data)?;
            if entry.mode == MODE_EXECUTABLE {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&abs_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&abs_path, perms)?;
            }
        }

        // Refresh stat data in the index entry so that subsequent
        // `stat_matches` calls see up-to-date values (prevents
        // spurious re-staging by `git add`).
        if let Ok(meta) = std::fs::symlink_metadata(&abs_path) {
            use std::os::unix::fs::MetadataExt;
            entry.ctime_sec = meta.ctime() as u32;
            entry.ctime_nsec = meta.ctime_nsec() as u32;
            entry.mtime_sec = meta.mtime() as u32;
            entry.mtime_nsec = meta.mtime_nsec() as u32;
            entry.dev = meta.dev() as u32;
            entry.ino = meta.ino() as u32;
            entry.uid = meta.uid();
            entry.gid = meta.gid();
            entry.size = meta.len() as u32;
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
