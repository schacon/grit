//! `grit am` — apply patches from mailbox-format files.
//!
//! Reads one or more mbox-format patch files (as produced by `git format-patch`)
//! and applies each patch as a new commit, preserving the original author,
//! date, and commit message from the email headers.
//!
//! Modes:
//! - `grit am <mbox>...` — apply patches from mbox files
//! - `grit am --continue` — continue after resolving conflicts
//! - `grit am --abort` — abort the current am session

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use grit_lib::config::ConfigSet;
use grit_lib::error::Error as GritError;
use grit_lib::index::Index;
use grit_lib::objects::{
    parse_commit, serialize_commit, CommitData, ObjectId, ObjectKind,
};
use grit_lib::repo::Repository;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::write_tree_from_index;

/// Arguments for `grit am`.
#[derive(Debug, ClapArgs)]
#[command(about = "Apply patches from mailbox")]
pub struct Args {
    /// Mbox file(s) containing patches.
    #[arg(value_name = "MBOX")]
    pub mbox: Vec<String>,

    /// Continue applying patches after resolving a conflict.
    #[arg(long = "continue")]
    pub r#continue: bool,

    /// Abort the current am session.
    #[arg(long = "abort")]
    pub abort: bool,

    /// Skip the current patch.
    #[arg(long = "skip")]
    pub skip: bool,

    /// Attempt three-way merge if patch doesn't apply cleanly.
    #[arg(short = '3', long = "3way")]
    pub three_way: bool,

    /// Quiet mode — suppress output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Do not apply the patch, just show what would be applied.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Read patches from stdin (default if no files given).
    #[arg(long = "stdin")]
    pub stdin: bool,
}

/// A parsed patch from an mbox message.
#[derive(Debug)]
struct MboxPatch {
    /// Author name + email (e.g. "Name <email>").
    author: String,
    /// Author date string (for the ident line).
    date: String,
    /// Commit message (subject + body).
    message: String,
    /// The unified diff portion.
    diff: String,
}

/// Run the `am` command.
/// Options threaded through the apply loop.
struct AmOptions {
    quiet: bool,
    three_way: bool,
}

pub fn run(args: Args) -> Result<()> {
    if args.abort {
        return do_abort();
    }
    if args.skip {
        return do_skip();
    }
    if args.r#continue {
        return do_continue(args.quiet);
    }

    if args.mbox.is_empty() && !args.stdin {
        return do_am_stdin(args);
    }
    if args.stdin {
        return do_am_stdin(args);
    }

    do_am(args)
}

// ── State directory ─────────────────────────────────────────────────
//
// .git/rebase-apply/  (shared with rebase, as git does)
//   applying          — marker that this is am, not rebase
//   orig-head         — original HEAD OID
//   patches/<N>       — individual parsed patches
//   current           — index (1-based) of current patch being applied
//   last              — total number of patches
//   next              — next patch to apply (1-based)

fn am_dir(git_dir: &Path) -> std::path::PathBuf {
    git_dir.join("rebase-apply")
}

fn is_am_in_progress(git_dir: &Path) -> bool {
    let dir = am_dir(git_dir);
    dir.exists() && dir.join("applying").exists()
}

// ── Main flow ───────────────────────────────────────────────────────

fn do_am(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if is_am_in_progress(git_dir) {
        bail!(
            "error: an am session is already in progress\n\
             hint: use \"grit am --continue\" to continue\n\
             hint: or \"grit am --abort\" to abort"
        );
    }

    // Read and parse all mbox files
    let mut all_patches = Vec::new();
    for mbox_path in &args.mbox {
        let content = fs::read_to_string(mbox_path)
            .with_context(|| format!("cannot read mbox file '{mbox_path}'"))?;
        let mut patches = parse_mbox(&content)?;
        all_patches.append(&mut patches);
    }

    if all_patches.is_empty() {
        eprintln!("Patch format detection failed."); std::process::exit(128);
    }

    if args.dry_run {
        for (i, patch) in all_patches.iter().enumerate() {
            let subject = patch.message.lines().next().unwrap_or("(no subject)");
            println!("Patch {}/{}: {}", i + 1, all_patches.len(), subject);
        }
        return Ok(());
    }

    // Save state
    let state_dir = am_dir(git_dir);
    fs::create_dir_all(state_dir.join("patches"))?;
    fs::write(state_dir.join("applying"), "")?;

    let head = resolve_head(git_dir)?;
    let head_oid = head.oid().map(|o| o.to_hex()).unwrap_or_default();
    fs::write(state_dir.join("orig-head"), &head_oid)?;
    // Save the raw HEAD content so abort can restore branch state
    let head_content = fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();
    fs::write(state_dir.join("head-name"), head_content.trim())?;
    fs::write(state_dir.join("last"), all_patches.len().to_string())?;
    fs::write(state_dir.join("next"), "1")?;

    // Write individual patches
    for (i, patch) in all_patches.iter().enumerate() {
        let patch_file = state_dir.join("patches").join((i + 1).to_string());
        let serialized = serialize_mbox_patch(patch);
        fs::write(&patch_file, serialized)?;
    }

    // Apply patches
    let opts = AmOptions {
        quiet: args.quiet,
        three_way: args.three_way,
    };
    apply_remaining(&repo, &opts)?;

    Ok(())
}

fn do_am_stdin(args: Args) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)
        .context("failed to read from stdin")?;

    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if is_am_in_progress(git_dir) {
        bail!(
            "error: an am session is already in progress\n\
             hint: use \"grit am --continue\" to continue\n\
             hint: or \"grit am --abort\" to abort"
        );
    }

    let all_patches = parse_mbox(&input)?;
    if all_patches.is_empty() {
        eprintln!("Patch format detection failed."); std::process::exit(128);
    }

    if args.dry_run {
        for (i, patch) in all_patches.iter().enumerate() {
            let subject = patch.message.lines().next().unwrap_or("(no subject)");
            println!("Patch {}/{}: {}", i + 1, all_patches.len(), subject);
        }
        return Ok(());
    }

    let state_dir = am_dir(git_dir);
    fs::create_dir_all(state_dir.join("patches"))?;
    fs::write(state_dir.join("applying"), "")?;

    let head = resolve_head(git_dir)?;
    let head_oid = head.oid().map(|o| o.to_hex()).unwrap_or_default();
    fs::write(state_dir.join("orig-head"), &head_oid)?;
    let head_content = fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();
    fs::write(state_dir.join("head-name"), head_content.trim())?;
    fs::write(state_dir.join("last"), all_patches.len().to_string())?;
    fs::write(state_dir.join("next"), "1")?;

    for (i, patch) in all_patches.iter().enumerate() {
        let patch_file = state_dir.join("patches").join((i + 1).to_string());
        let serialized = serialize_mbox_patch(patch);
        fs::write(&patch_file, serialized)?;
    }

    let opts = AmOptions {
        quiet: args.quiet,
        three_way: args.three_way,
    };
    apply_remaining(&repo, &opts)?;
    Ok(())
}

/// Apply all remaining patches.
fn apply_remaining(repo: &Repository, opts: &AmOptions) -> Result<()> {
    let git_dir = &repo.git_dir;
    let state_dir = am_dir(git_dir);

    let last: usize = fs::read_to_string(state_dir.join("last"))?.trim().parse()?;
    let mut next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;

    while next <= last {
        let patch_file = state_dir.join("patches").join(next.to_string());
        let serialized = fs::read_to_string(&patch_file)?;
        let patch = deserialize_mbox_patch(&serialized)?;

        fs::write(state_dir.join("current"), next.to_string())?;

        match apply_one_patch(repo, &patch, opts.three_way) {
            Ok(()) => {
                let subject = patch.message.lines().next().unwrap_or("");
                if !opts.quiet {
                    eprintln!("Applying: {}", subject);
                }
                next += 1;
                fs::write(state_dir.join("next"), next.to_string())?;
            }
            Err(e) => {
                let subject = patch.message.lines().next().unwrap_or("");
                eprintln!(
                    "error: patch failed: {}\n\
                     Applying: {}\n\
                     hint: Fix the patch and run \"grit am --continue\".\n\
                     hint: To abort, run \"grit am --abort\".",
                    e, subject
                );
                std::process::exit(1);
            }
        }
    }

    // All patches applied — cleanup
    cleanup_am_state(git_dir);
    Ok(())
}

/// Apply a single mbox patch: apply the diff, then create a commit.
fn apply_one_patch(repo: &Repository, patch: &MboxPatch, _three_way: bool) -> Result<()> {
    let git_dir = &repo.git_dir;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cannot apply patches in a bare repository"))?;

    // Check if the index is dirty (has changes compared to HEAD tree)
    {
        let index = load_index(repo)?;
        let head = resolve_head(git_dir)?;
        if let Some(head_oid) = head.oid() {
            let obj = repo.odb.read(head_oid)?;
            let commit = parse_commit(&obj.data)?;
            let head_entries = tree_to_index_entries(repo, &commit.tree, "")?;
            // Compare index entries with HEAD tree entries
            if index.entries.len() != head_entries.len() ||
                index.entries.iter().zip(head_entries.iter()).any(|(a, b)| a.oid != b.oid || a.path != b.path) {
                bail!("your local changes would be overwritten by am.\n\
                       Please commit your changes or stash them before you apply patches.");
            }
        }
    }

    // Reject patches with no diff section
    if patch.diff.is_empty() {
        bail!("patch does not contain a valid diff");
    }

    // Apply the diff to the working tree and collect affected paths
    let affected_paths = apply_patch_to_worktree(work_tree, &patch.diff)?;

    // Stage only the files that the patch touched
    stage_affected_files(repo, &affected_paths)?;

    // Create commit
    let index = load_index(repo)?;

    // Check for conflicts
    if index.entries.iter().any(|e| e.stage() != 0) {
        // Save message for --continue
        fs::write(git_dir.join("MERGE_MSG"), &patch.message)?;
        bail!("patch has conflicts");
    }

    create_am_commit(repo, &index, patch)?;

    Ok(())
}

/// Apply a unified diff to the working tree files.
/// Returns the list of affected relative paths.
fn apply_patch_to_worktree(work_tree: &Path, diff: &str) -> Result<Vec<String>> {
    // Parse the diff into file patches using the same logic as `grit apply`
    let file_patches = parse_patch(diff)?;
    let mut affected = Vec::new();

    for fp in &file_patches {
        let path_str = fp
            .effective_path()
            .ok_or_else(|| anyhow::anyhow!("patch has no file path"))?;
        let rel_path = strip_components(path_str, 1);
        let path = work_tree.join(&rel_path);

        if fp.is_rename {
            // Handle rename: old path is removed, new path is added
            if let Some(old) = &fp.old_path {
                let old_rel = strip_components(old, 0);
                let old_abs = work_tree.join(&old_rel);
                if old_abs.exists() {
                    // Read old content, apply hunks if any, write to new path
                    let new_rel = fp.new_path.as_deref().map(|p| strip_components(p, 0)).unwrap_or_else(|| rel_path.clone());
                    let new_abs = work_tree.join(&new_rel);
                    if let Some(parent) = new_abs.parent() {
                        if !parent.as_os_str().is_empty() && !parent.exists() {
                            fs::create_dir_all(parent)?;
                        }
                    }
                    let old_content = fs::read_to_string(&old_abs)
                        .with_context(|| format!("cannot read {}", old_abs.display()))?;
                    let new_content = if fp.hunks.is_empty() {
                        old_content
                    } else {
                        apply_hunks(&old_content, &fp.hunks)
                            .with_context(|| format!("failed to apply patch to {}", old_abs.display()))?
                    };
                    fs::write(&new_abs, new_content.as_bytes())?;
                    fs::remove_file(&old_abs)?;
                    affected.push(old_rel);
                    affected.push(new_rel);
                }
            }
            continue;
        }

        affected.push(rel_path.clone());

        if fp.is_deleted {
            if path.exists() {
                fs::remove_file(&path)?;
            }
            continue;
        }

        if fp.is_new {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let content = apply_hunks("", &fp.hunks)?;
            fs::write(&path, content.as_bytes())?;
            #[cfg(unix)]
            if fp.new_mode.as_deref().map_or(false, |m| m == "100755") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            }
            continue;
        }

        // Modify existing file
        let old_content = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;

        if fp.hunks.is_empty() {
            #[cfg(unix)]
            if let Some(mode) = fp.new_mode.as_deref() {
                use std::os::unix::fs::PermissionsExt;
                let perm = if mode == "100755" { 0o755 } else { 0o644 };
                fs::set_permissions(&path, fs::Permissions::from_mode(perm))?;
            }
            continue;
        }

        let new_content = apply_hunks(&old_content, &fp.hunks)
            .with_context(|| format!("failed to apply patch to {}", path.display()))?;
        fs::write(&path, new_content.as_bytes())?;
    }

    Ok(affected)
}

/// Stage only the files affected by the patch into the index.
fn stage_affected_files(repo: &Repository, affected_paths: &[String]) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no work tree"))?;

    let mut index = load_index(repo)?;

    for rel_path in affected_paths {
        let abs = work_tree.join(rel_path);
        if !abs.exists() && !abs.is_symlink() {
            // File was deleted — remove from index
            let path_bytes = rel_path.as_bytes().to_vec();
            index.entries.retain(|e| e.path != path_bytes);
            continue;
        }

        if abs.is_dir() {
            continue;
        }

        let content = fs::read(&abs)?;
        let oid = repo.odb.write(ObjectKind::Blob, &content)?;
        let metadata = fs::metadata(&abs)?;

        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = metadata.permissions().mode();
                if perms & 0o111 != 0 {
                    0o100755u32
                } else {
                    0o100644u32
                }
            }
            #[cfg(not(unix))]
            {
                0o100644u32
            }
        };

        let path_bytes = rel_path.as_bytes().to_vec();
        let size = content.len() as u32;

        let entry = grit_lib::index::IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size,
            oid,
            flags: (path_bytes.len().min(0xFFF)) as u16,
            flags_extended: None,
            path: path_bytes,
        };
        index.add_or_replace(entry);
    }

    index.sort();
    index.write(&repo.index_path())?;
    Ok(())
}

/// Stage all working tree changes to the index.
fn stage_all_changes(repo: &Repository) -> Result<()> {
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no work tree"))?;

    let mut index = load_index(repo)?;

    // Walk the working tree and update index
    // For simplicity, we re-read all tracked files + discover new ones
    let mut worktree_files = HashSet::new();
    collect_files(work_tree, work_tree, &mut worktree_files)?;

    // Remove entries for files that no longer exist
    index.entries.retain(|e| {
        let path = String::from_utf8_lossy(&e.path).into_owned();
        let abs = work_tree.join(&path);
        if !abs.exists() && !abs.is_symlink() {
            false
        } else {
            true
        }
    });

    // Update/add entries for existing files
    for rel_path in &worktree_files {
        let abs = work_tree.join(rel_path);
        if abs.is_dir() {
            continue;
        }

        let content = fs::read(&abs)?;
        let oid = repo.odb.write(ObjectKind::Blob, &content)?;
        let metadata = fs::metadata(&abs)?;

        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = metadata.permissions().mode();
                if perms & 0o111 != 0 {
                    0o100755u32
                } else {
                    0o100644u32
                }
            }
            #[cfg(not(unix))]
            {
                0o100644u32
            }
        };

        let path_bytes = rel_path.as_bytes().to_vec();
        let size = content.len() as u32;

        let entry = grit_lib::index::IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size,
            oid,
            flags: (path_bytes.len().min(0xFFF)) as u16,
            flags_extended: None,
            path: path_bytes,
        };
        index.add_or_replace(entry);
    }

    index.sort();
    index.write(&repo.index_path())?;
    Ok(())
}

/// Recursively collect relative file paths from a directory.
fn collect_files(root: &Path, dir: &Path, out: &mut HashSet<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip .git directory
        if name_str == ".git" {
            continue;
        }

        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !rel.is_empty() {
                out.insert(rel);
            }
        }
    }
    Ok(())
}

/// Create a commit from the current index using the patch metadata.
fn create_am_commit(repo: &Repository, index: &Index, patch: &MboxPatch) -> Result<()> {
    let git_dir = &repo.git_dir;
    let tree_oid = write_tree_from_index(&repo.odb, index, "")?;

    let head = resolve_head(git_dir)?;
    let mut parents = Vec::new();
    if let Some(head_oid) = head.oid() {
        parents.push(*head_oid);
    }

    let config = ConfigSet::load(Some(git_dir), true)?;
    let now = time::OffsetDateTime::now_utc();
    let committer = resolve_identity(&config, "COMMITTER")?;

    // Build author ident from patch metadata
    let author_ident = if !patch.author.is_empty() && !patch.date.is_empty() {
        format!("{} {}", patch.author, patch.date)
    } else if !patch.author.is_empty() {
        let epoch = now.unix_timestamp();
        format!("{} {} +0000", patch.author, epoch)
    } else {
        format_ident(&committer, now)
    };

    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author: author_ident,
        committer: format_ident(&committer, now),
        encoding: None,
        message: patch.message.clone(),
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD
    update_head(git_dir, &head, &commit_oid)?;

    Ok(())
}

// ── --continue ──────────────────────────────────────────────────────

fn do_skip() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_am_in_progress(git_dir) {
        bail!("error: no am session in progress");
    }

    let state_dir = am_dir(git_dir);
    let next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
    let last: usize = fs::read_to_string(state_dir.join("last"))?.trim().parse()?;

    if next > last {
        // Nothing left to skip — just cleanup
        cleanup_am_state(git_dir);
        return Ok(());
    }

    // Reset working tree to HEAD state (undo partial apply)
    let head = resolve_head(git_dir)?;
    if let Some(head_oid) = head.oid() {
        let obj = repo.odb.read(head_oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let mut index = Index::new();
        index.entries = entries;
        index.sort();
        index.write(&repo.index_path())?;

        if let Some(wt) = &repo.work_tree {
            checkout_index_to_worktree(&repo, wt, &index)?;
        }
    }

    // Advance past the skipped patch
    fs::write(state_dir.join("next"), (next + 1).to_string())?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    let opts = AmOptions {
        quiet: false,
        three_way: false,
    };
    apply_remaining(&repo, &opts)?;

    Ok(())
}

fn do_continue(quiet: bool) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    if !is_am_in_progress(git_dir) {
        bail!("error: no am session in progress");
    }

    // The user should have already staged their resolution via 'git add'
    let index = load_index(&repo)?;
    if index.entries.iter().any(|e| e.stage() != 0) {
        bail!(
            "error: you still have unmerged files\n\
             hint: resolve conflicts, stage with 'grit add', then 'grit am --continue'"
        );
    }

    let state_dir = am_dir(git_dir);
    let current: usize = fs::read_to_string(state_dir.join("current"))?.trim().parse()?;
    let patch_file = state_dir.join("patches").join(current.to_string());
    let serialized = fs::read_to_string(&patch_file)?;
    let patch = deserialize_mbox_patch(&serialized)?;

    // Read message (might have been edited)
    let message = match fs::read_to_string(git_dir.join("MERGE_MSG")) {
        Ok(m) => m,
        Err(_) => patch.message.clone(),
    };

    let patched = MboxPatch {
        message,
        ..patch
    };

    create_am_commit(&repo, &index, &patched)?;

    let subject = patched.message.lines().next().unwrap_or("");
    if !quiet {
        eprintln!("Applying: {}", subject);
    }

    // Advance next
    let next: usize = fs::read_to_string(state_dir.join("next"))?.trim().parse()?;
    fs::write(state_dir.join("next"), (next + 1).to_string())?;
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));

    // Continue with remaining
    let opts = AmOptions {
        quiet,
        three_way: false,
    };
    apply_remaining(&repo, &opts)?;

    Ok(())
}

// ── --abort ─────────────────────────────────────────────────────────

fn do_abort() -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let git_dir = &repo.git_dir;

    let state_dir = am_dir(git_dir);
    if !state_dir.exists() {
        bail!("error: no am session in progress");
    }

    // Handle stray directory (no applying marker or no state files)
    if !state_dir.join("applying").exists() || !state_dir.join("orig-head").exists() {
        let _ = fs::remove_dir_all(&state_dir);
        return Ok(());
    }

    let state_dir = am_dir(git_dir);
    let orig_head_hex = fs::read_to_string(state_dir.join("orig-head"))?;
    let orig_head_hex = orig_head_hex.trim();

    if !orig_head_hex.is_empty() {
        let orig_oid = ObjectId::from_hex(orig_head_hex)?;

        // Restore to original HEAD
        let obj = repo.odb.read(&orig_oid)?;
        let commit = parse_commit(&obj.data)?;
        let entries = tree_to_index_entries(&repo, &commit.tree, "")?;
        let mut index = Index::new();
        index.entries = entries;
        index.sort();
        index.write(&repo.index_path())?;

        if let Some(wt) = &repo.work_tree {
            checkout_index_to_worktree(&repo, wt, &index)?;
        }

        // Restore HEAD — use saved head-name to restore branch state
        let head_name = fs::read_to_string(state_dir.join("head-name"))
            .unwrap_or_default();
        let head_name = head_name.trim();
        if let Some(refname) = head_name.strip_prefix("ref: ") {
            // Was on a branch — restore the ref
            fs::write(git_dir.join("HEAD"), format!("{}\n", head_name))?;
            let ref_path = git_dir.join(refname);
            if let Some(parent) = ref_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&ref_path, format!("{}\n", orig_oid.to_hex()))?;
        } else {
            // Was detached
            fs::write(git_dir.join("HEAD"), format!("{}\n", orig_oid.to_hex()))?;
        }
    }

    cleanup_am_state(git_dir);
    eprintln!("am session aborted.");

    Ok(())
}

// ── Cleanup ─────────────────────────────────────────────────────────

fn cleanup_am_state(git_dir: &Path) {
    let state_dir = am_dir(git_dir);
    // Only clean up if this is an am session (has "applying" marker)
    if state_dir.join("applying").exists() {
        let _ = fs::remove_dir_all(&state_dir);
    }
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
}

// ── Mbox parsing ────────────────────────────────────────────────────

/// Parse an mbox file into individual patches.
fn parse_mbox(input: &str) -> Result<Vec<MboxPatch>> {
    let mut patches = Vec::new();
    let mut lines = input.lines().peekable();

    while lines.peek().is_some() {
        // Skip to next "From " line (mbox separator)
        // Or if we're at the start and there's no "From " line, treat as single patch
        let mut _in_headers = false;
        let mut author = String::new();
        let mut date = String::new();
        let mut subject = String::new();
        let _body = String::new();
        let mut found_from = false;

        // Look for "From " separator line
        while let Some(&line) = lines.peek() {
            if line.starts_with("From ") && line.len() > 5 {
                found_from = true;
                lines.next(); // consume "From " line
                break;
            }
            // If we haven't found any "From " line yet and we see headers, treat as raw patch
            if !found_from && (line.starts_with("From:") || line.starts_with("Subject:") || line.starts_with("Date:")) {
                found_from = true;
                break;
            }
            if !found_from {
                lines.next(); // skip non-header lines before first message
                continue;
            }
            break;
        }

        if !found_from && lines.peek().is_none() {
            break;
        }

        // Parse headers
        _in_headers = true;
        let mut last_header = String::new();

        while let Some(&line) = lines.peek() {
            if line.is_empty() {
                lines.next();
                _in_headers = false;
                break;
            }
            // Continuation line (starts with whitespace)
            if (line.starts_with(' ') || line.starts_with('\t')) && !last_header.is_empty() {
                if last_header == "subject" {
                    subject.push(' ');
                    subject.push_str(line.trim());
                }
                lines.next();
                continue;
            }

            if let Some(value) = line.strip_prefix("From: ") {
                author = value.trim().to_string();
                last_header = "from".to_string();
            } else if let Some(value) = line.strip_prefix("Date: ") {
                date = value.trim().to_string();
                last_header = "date".to_string();
            } else if let Some(value) = line.strip_prefix("Subject: ") {
                // Strip [PATCH ...] prefix
                let subj = strip_patch_prefix(value.trim());
                subject = subj;
                last_header = "subject".to_string();
            } else {
                last_header = String::new();
            }
            lines.next();
        }

        // Parse body (everything until "---" separator or diff start)
        let mut in_diff = false;
        let mut body_lines = Vec::new();
        let mut diff_lines = Vec::new();

        while let Some(&line) = lines.peek() {
            // Check for next mbox message
            if line.starts_with("From ") && line.len() > 5 && !diff_lines.is_empty() {
                break;
            }

            if !in_diff {
                if line == "---" {
                    // Separator between message body and diffstat/diff
                    lines.next();
                    // Now skip diffstat lines until we hit "diff --git"
                    while let Some(&l) = lines.peek() {
                        if l.starts_with("diff --git ") {
                            in_diff = true;
                            break;
                        }
                        if l.starts_with("From ") && l.len() > 5 {
                            break;
                        }
                        lines.next();
                    }
                    continue;
                }
                if line.starts_with("diff --git ") {
                    in_diff = true;
                    // Don't consume — fall through to diff section
                } else {
                    body_lines.push(line);
                    lines.next();
                    continue;
                }
            }

            if in_diff {
                // Collect diff lines until "-- " (signature separator) or next message
                if line == "-- " || line == "-- \n" {
                    lines.next();
                    // Skip remaining signature lines
                    while let Some(&l) = lines.peek() {
                        if l.starts_with("From ") && l.len() > 5 {
                            break;
                        }
                        lines.next();
                    }
                    break;
                }
                if line.starts_with("From ") && line.len() > 5 {
                    break;
                }
                diff_lines.push(line);
                lines.next();
            }
        }

        // Build message from subject + body
        let body_str = body_lines.join("\n").trim().to_string();
        let message = if body_str.is_empty() {
            format!("{}\n", subject)
        } else {
            format!("{}\n\n{}\n", subject, body_str)
        };

        // Parse author into "Name <email>" format and extract date
        let author_ident = parse_author_ident(&author, &date);

        let mut diff_section = diff_lines.join("\n");
        if !diff_section.is_empty() {
            diff_section.push('\n');
        }

        if !subject.is_empty() || !diff_section.is_empty() {
            patches.push(MboxPatch {
                author: author_ident.0,
                date: author_ident.1,
                message,
                diff: diff_section,
            });
        }
    }

    Ok(patches)
}

/// Strip "[PATCH n/m] " or "[PATCH] " prefix from subject.
fn strip_patch_prefix(subject: &str) -> String {
    if subject.starts_with('[') {
        if let Some(end) = subject.find(']') {
            let rest = subject[end + 1..].trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    subject.to_string()
}

/// Parse "Name <email>" and date string into (author_ident, epoch_offset).
fn parse_author_ident(author: &str, date: &str) -> (String, String) {
    // Try to parse the date into epoch format
    let epoch_date = parse_date_to_epoch(date);
    (author.to_string(), epoch_date)
}

/// Try to parse various date formats into "epoch offset" format.
fn parse_date_to_epoch(date: &str) -> String {
    if date.is_empty() {
        return String::new();
    }

    // Already in "epoch offset" format?
    let parts: Vec<&str> = date.split_whitespace().collect();
    if parts.len() == 2 {
        if parts[0].parse::<i64>().is_ok() {
            return date.to_string();
        }
    }

    // Try RFC 2822-like: "Thu, 07 Apr 2005 22:14:13 -0700"
    if let Some(parsed) = parse_rfc2822_date(date) {
        return parsed;
    }

    // Fall back: just use the date string as-is
    date.to_string()
}

/// Parse an RFC 2822-style date into "epoch offset" format.
fn parse_rfc2822_date(date: &str) -> Option<String> {
    // Format: "Day, DD Mon YYYY HH:MM:SS +/-HHMM" or without the day prefix
    let trimmed = date.trim();

    // Extract the timezone offset (last token)
    let (date_part, tz_str) = {
        let parts: Vec<&str> = trimmed.rsplitn(2, ' ').collect();
        if parts.len() != 2 {
            return None;
        }
        (parts[1], parts[0])
    };

    // Parse timezone offset like +0700 or -0700
    if tz_str.len() != 5 {
        return None;
    }
    let tz_sign = match tz_str.chars().next()? {
        '+' => 1i32,
        '-' => -1i32,
        _ => return None,
    };
    let tz_hours: i32 = tz_str[1..3].parse().ok()?;
    let tz_mins: i32 = tz_str[3..5].parse().ok()?;
    let tz_offset_secs = tz_sign * (tz_hours * 3600 + tz_mins * 60);

    // Strip leading "Day, " if present
    let date_str = if date_part.contains(',') {
        let (_, rest) = date_part.split_once(',')?;
        rest.trim()
    } else {
        date_part.trim()
    };

    // Parse "DD Mon YYYY HH:MM:SS"
    let tokens: Vec<&str> = date_str.split_whitespace().collect();
    if tokens.len() < 4 {
        return None;
    }

    let day: u32 = tokens[0].parse().ok()?;
    let month = match tokens[1].to_lowercase().as_str() {
        "jan" => 1u32, "feb" => 2, "mar" => 3, "apr" => 4,
        "may" => 5, "jun" => 6, "jul" => 7, "aug" => 8,
        "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => return None,
    };
    let year: i32 = tokens[2].parse().ok()?;
    let time_parts: Vec<&str> = tokens[3].split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = if time_parts.len() > 2 { time_parts[2].parse().ok()? } else { 0 };

    // Convert to Unix timestamp
    // Days from year 0 to year, then month/day, then subtract Unix epoch
    let epoch = datetime_to_epoch(year, month, day, hour, min, sec, tz_offset_secs)?;

    Some(format!("{} {}", epoch, tz_str))
}

/// Convert a date to Unix epoch seconds.
fn datetime_to_epoch(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32, tz_offset_secs: i32) -> Option<i64> {
    // Use a simple calculation
    let m = if month <= 2 { month + 12 } else { month };
    let y = if month <= 2 { year - 1 } else { year };

    // Julian Day Number
    let jdn = (day as i64) + (153 * (m as i64 - 3) + 2) / 5
        + 365 * (y as i64) + (y as i64) / 4 - (y as i64) / 100 + (y as i64) / 400 + 1721119;

    // Unix epoch = JDN of 1970-01-01 = 2440588
    let days_since_epoch = jdn - 2440588;
    let secs = days_since_epoch * 86400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64);
    let utc_secs = secs - (tz_offset_secs as i64);

    Some(utc_secs)
}

/// Serialize an MboxPatch for storage in the state directory.
fn serialize_mbox_patch(patch: &MboxPatch) -> String {
    let mut out = String::new();
    out.push_str(&format!("Author: {}\n", patch.author));
    out.push_str(&format!("Date: {}\n", patch.date));
    out.push_str(&format!("Message-Length: {}\n", patch.message.len()));
    out.push_str(&format!("Diff-Length: {}\n", patch.diff.len()));
    out.push('\n');
    out.push_str(&patch.message);
    out.push_str(&patch.diff);
    out
}

/// Deserialize an MboxPatch from state directory storage.
fn deserialize_mbox_patch(data: &str) -> Result<MboxPatch> {
    let mut author = String::new();
    let mut date = String::new();
    let mut msg_len = 0usize;
    let mut diff_len = 0usize;

    let mut lines = data.lines();
    for line in &mut lines {
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Author: ") {
            author = v.to_string();
        } else if let Some(v) = line.strip_prefix("Date: ") {
            date = v.to_string();
        } else if let Some(v) = line.strip_prefix("Message-Length: ") {
            msg_len = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Diff-Length: ") {
            diff_len = v.parse().unwrap_or(0);
        }
    }

    // Remaining content is message + diff
    let remaining: String = lines.collect::<Vec<&str>>().join("\n");
    // Add back the newline that .lines() stripped
    let remaining = if data.ends_with('\n') && !remaining.ends_with('\n') {
        format!("{remaining}\n")
    } else {
        remaining
    };

    let message = if msg_len > 0 && msg_len <= remaining.len() {
        remaining[..msg_len].to_string()
    } else {
        remaining.clone()
    };

    let diff = if diff_len > 0 && msg_len + diff_len <= remaining.len() {
        remaining[msg_len..msg_len + diff_len].to_string()
    } else if msg_len < remaining.len() {
        remaining[msg_len..].to_string()
    } else {
        String::new()
    };

    Ok(MboxPatch {
        author,
        date,
        message,
        diff,
    })
}

// ── Patch parsing (subset of apply.rs logic) ────────────────────────

#[derive(Debug, Clone)]
struct FilePatch {
    old_path: Option<String>,
    new_path: Option<String>,
    old_mode: Option<String>,
    new_mode: Option<String>,
    is_new: bool,
    is_deleted: bool,
    is_rename: bool,
    hunks: Vec<Hunk>,
}

impl FilePatch {
    fn effective_path(&self) -> Option<&str> {
        if self.is_deleted {
            return self.old_path.as_deref().filter(|p| *p != "/dev/null")
                .or(self.new_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        if self.is_new {
            return self.new_path.as_deref().filter(|p| *p != "/dev/null")
                .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"));
        }
        self.new_path.as_deref().filter(|p| *p != "/dev/null")
            .or(self.old_path.as_deref().filter(|p| *p != "/dev/null"))
    }
}

#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    _old_count: usize,
    _new_start: usize,
    _new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
    NoNewline,
}

fn parse_patch(input: &str) -> Result<Vec<FilePatch>> {
    let lines: Vec<&str> = input.lines().collect();
    let mut patches = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("diff --git ") {
            let mut fp = FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_deleted: false,
                is_rename: false,
                hunks: Vec::new(),
            };

            let rest = &lines[i]["diff --git ".len()..];
            if let Some((a, b)) = split_diff_git_paths(rest) {
                fp.old_path = Some(a);
                fp.new_path = Some(b);
            }
            i += 1;

            while i < lines.len()
                && !lines[i].starts_with("--- ")
                && !lines[i].starts_with("diff --git ")
                && !lines[i].starts_with("@@ ")
            {
                let line = lines[i];
                if let Some(val) = line.strip_prefix("old mode ") {
                    fp.old_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("new mode ") {
                    fp.new_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("new file mode ") {
                    fp.is_new = true;
                    fp.new_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("deleted file mode ") {
                    fp.is_deleted = true;
                    fp.old_mode = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("rename from ") {
                    fp.is_rename = true;
                    fp.old_path = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("rename to ") {
                    fp.is_rename = true;
                    fp.new_path = Some(val.to_string());
                }
                i += 1;
            }

            if i < lines.len() && lines[i].starts_with("--- ") {
                let old_p = &lines[i]["--- ".len()..];
                fp.old_path = Some(old_p.to_string());
                i += 1;
                if i < lines.len() && lines[i].starts_with("+++ ") {
                    let new_p = &lines[i]["+++ ".len()..];
                    fp.new_path = Some(new_p.to_string());
                    i += 1;
                }
            }

            while i < lines.len() && lines[i].starts_with("@@ ") {
                let (hunk, next_i) = parse_hunk(&lines, i)?;
                fp.hunks.push(hunk);
                i = next_i;
            }

            patches.push(fp);
        } else {
            i += 1;
        }
    }

    Ok(patches)
}

fn split_diff_git_paths(s: &str) -> Option<(String, String)> {
    // Keep raw paths (with a/ b/ prefix) so -p<n> stripping works correctly.
    if let Some(pos) = s.find(" b/") {
        let a = &s[..pos];
        let b = &s[pos + 1..];
        return Some((a.to_string(), b.to_string()));
    }
    if s.starts_with("a/") {
        if let Some(pos) = s.find(" /dev/null") {
            let a = &s[..pos];
            return Some((a.to_string(), "/dev/null".to_string()));
        }
    }
    if let Some(b) = s.strip_prefix("/dev/null ") {
        return Some(("/dev/null".to_string(), b.to_string()));
    }
    None
}

fn strip_ab_prefix(p: &str) -> String {
    if p == "/dev/null" {
        return p.to_string();
    }
    if p.starts_with("a/") || p.starts_with("b/") {
        return p[2..].to_string();
    }
    p.to_string()
}

fn strip_components(path: &str, n: usize) -> String {
    if n == 0 {
        return path.to_string();
    }
    let mut remaining = path;
    for _ in 0..n {
        if let Some(pos) = remaining.find('/') {
            remaining = &remaining[pos + 1..];
        } else {
            return remaining.to_string();
        }
    }
    remaining.to_string()
}

fn parse_hunk(lines: &[&str], start: usize) -> Result<(Hunk, usize)> {
    let header = lines[start];
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)
        .with_context(|| format!("invalid hunk header: {header}"))?;

    let mut hunk = Hunk {
        old_start,
        _old_count: old_count,
        _new_start: new_start,
        _new_count: new_count,
        lines: Vec::new(),
    };

    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("@@ ") || line.starts_with("diff --git ") {
            break;
        }
        if let Some(rest) = line.strip_prefix('+') {
            hunk.lines.push(HunkLine::Add(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix('-') {
            hunk.lines.push(HunkLine::Remove(rest.to_string()));
        } else if line.is_empty() {
            hunk.lines.push(HunkLine::Context(String::new()));
        } else if let Some(rest) = line.strip_prefix(' ') {
            hunk.lines.push(HunkLine::Context(rest.to_string()));
        } else if line.starts_with('\\') {
            hunk.lines.push(HunkLine::NoNewline);
        } else {
            break;
        }
        i += 1;
    }

    Ok((hunk, i))
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize, usize, usize)> {
    let trimmed = line.trim_start_matches('@').trim_start();
    let end = trimmed.find(" @@").unwrap_or(trimmed.len());
    let range_part = &trimmed[..end];

    let parts: Vec<&str> = range_part.split_whitespace().collect();
    if parts.len() < 2 {
        bail!("expected old and new range in hunk header");
    }

    let (old_start, old_count) = parse_range(parts[0].trim_start_matches('-'))?;
    let (new_start, new_count) = parse_range(parts[1].trim_start_matches('+'))?;

    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(s: &str) -> Result<(usize, usize)> {
    if let Some((start_s, count_s)) = s.split_once(',') {
        Ok((start_s.parse()?, count_s.parse()?))
    } else {
        let n: usize = s.parse()?;
        Ok((n, 1))
    }
}

fn apply_hunks(old_content: &str, hunks: &[Hunk]) -> Result<String> {
    let has_trailing_newline = old_content.is_empty() || old_content.ends_with('\n');
    let old_lines: Vec<&str> = if old_content.is_empty() {
        Vec::new()
    } else {
        old_content.lines().collect()
    };

    let mut result: Vec<String> = Vec::new();
    let mut old_idx: usize = 0;

    for hunk in hunks {
        let hunk_start = if hunk.old_start == 0 { 0 } else { hunk.old_start - 1 };

        while old_idx < hunk_start && old_idx < old_lines.len() {
            result.push(old_lines[old_idx].to_string());
            old_idx += 1;
        }

        for hl in &hunk.lines {
            match hl {
                HunkLine::Context(s) => {
                    if old_idx < old_lines.len() {
                        if old_lines[old_idx] != s.as_str() {
                            bail!(
                                "context mismatch at line {}: expected {:?}, got {:?}",
                                old_idx + 1, s, old_lines[old_idx]
                            );
                        }
                        old_idx += 1;
                    }
                    result.push(s.clone());
                }
                HunkLine::Remove(s) => {
                    if old_idx < old_lines.len() {
                        if old_lines[old_idx] != s.as_str() {
                            bail!(
                                "remove mismatch at line {}: expected {:?}, got {:?}",
                                old_idx + 1, s, old_lines[old_idx]
                            );
                        }
                        old_idx += 1;
                    }
                }
                HunkLine::Add(s) => {
                    result.push(s.clone());
                }
                HunkLine::NoNewline => {}
            }
        }
    }

    while old_idx < old_lines.len() {
        result.push(old_lines[old_idx].to_string());
        old_idx += 1;
    }

    if result.is_empty() {
        return Ok(String::new());
    }

    let ends_no_newline = hunks.last().map_or(false, |h| {
        let mut last_was_add = false;
        let mut saw_no_newline_after_add = false;
        for hl in &h.lines {
            match hl {
                HunkLine::Add(_) => {
                    last_was_add = true;
                    saw_no_newline_after_add = false;
                }
                HunkLine::NoNewline if last_was_add => {
                    saw_no_newline_after_add = true;
                }
                HunkLine::Remove(_) => {
                    last_was_add = false;
                }
                HunkLine::Context(_) => {
                    last_was_add = false;
                    saw_no_newline_after_add = false;
                }
                _ => {}
            }
        }
        saw_no_newline_after_add
    });

    let mut out = result.join("\n");
    if !ends_no_newline && (has_trailing_newline || !hunks.is_empty()) {
        out.push('\n');
    }

    Ok(out)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn load_index(repo: &Repository) -> Result<Index> {
    let index_path = repo.index_path();
    match Index::load(&index_path) {
        Ok(idx) => Ok(idx),
        Err(GritError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::new()),
        Err(e) => Err(e.into()),
    }
}

fn resolve_identity(config: &ConfigSet, kind: &str) -> Result<(String, String)> {
    let name_var = format!("GIT_{kind}_NAME");
    let email_var = format!("GIT_{kind}_EMAIL");

    let name = std::env::var(&name_var)
        .ok()
        .or_else(|| config.get("user.name"))
        .unwrap_or_else(|| "Unknown".to_owned());
    let email = std::env::var(&email_var)
        .ok()
        .or_else(|| config.get("user.email"))
        .unwrap_or_default();

    Ok((name, email))
}

fn format_ident(ident: &(String, String), now: time::OffsetDateTime) -> String {
    let (name, email) = ident;
    // Respect GIT_COMMITTER_DATE if set
    let timestamp = if let Ok(date) = std::env::var("GIT_COMMITTER_DATE") {
        date
    } else {
        let epoch = now.unix_timestamp();
        let offset = now.offset();
        let hours = offset.whole_hours();
        let minutes = offset.minutes_past_hour().unsigned_abs();
        format!("{epoch} {hours:+03}{minutes:02}")
    };
    format!("{name} <{email}> {timestamp}")
}

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

fn tree_to_index_entries(
    repo: &Repository,
    oid: &ObjectId,
    prefix: &str,
) -> Result<Vec<grit_lib::index::IndexEntry>> {
    use grit_lib::objects::parse_tree;
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
            result.push(grit_lib::index::IndexEntry {
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

fn checkout_index_to_worktree(
    repo: &Repository,
    work_tree: &Path,
    index: &Index,
) -> Result<()> {
    use grit_lib::index::{MODE_EXECUTABLE, MODE_SYMLINK};

    for entry in &index.entries {
        let path_str = String::from_utf8_lossy(&entry.path).into_owned();
        let abs_path = work_tree.join(&path_str);

        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let obj = repo.odb.read(&entry.oid)?;

        if entry.mode == MODE_SYMLINK {
            let target = String::from_utf8(obj.data)
                .map_err(|_| anyhow::anyhow!("symlink not UTF-8"))?;
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
