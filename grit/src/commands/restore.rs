//! `grit restore` — restore working tree files.
//!
//! Restores specified paths in the working tree or the index from a given
//! source (index, HEAD, or an explicit tree-ish).  Unlike `reset`, this
//! command does **not** move `HEAD`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::refs;
use grit_lib::repo::Repository;
use std::path::{Path, PathBuf};

/// Arguments for `grit restore`.
#[derive(Debug, ClapArgs)]
#[command(about = "Restore working tree files")]
pub struct Args {
    /// Restore the index (unstage changes).  Default source when used alone is HEAD.
    #[arg(short = 'S', long = "staged")]
    pub staged: bool,

    /// Restore the working tree (the default when neither flag is given).
    /// Default source when used alone is the index.
    #[arg(short = 'W', long = "worktree")]
    pub worktree: bool,

    /// Use this tree-ish as the restore source instead of the index or HEAD.
    #[arg(short = 's', long = "source", value_name = "tree-ish")]
    pub source: Option<String>,

    /// When restoring the working tree from the index, skip unmerged (conflicted)
    /// entries instead of aborting.
    #[arg(long = "ignore-unmerged")]
    pub ignore_unmerged: bool,

    /// Suppress progress messages.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Paths to restore.  Use `.` to restore all tracked files.
    #[arg(required = true)]
    pub pathspec: Vec<String>,
}

/// Run the `restore` command.
pub fn run(args: Args) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let work_tree = repo
        .work_tree
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("this operation must be run in a work tree"))?
        .to_path_buf();

    // Determine which targets to restore.  If the user specified neither,
    // default to worktree only.
    let restore_staged = args.staged;
    let restore_worktree = args.worktree || !args.staged;

    let index_path = repo.index_path();
    let mut index = Index::load(&index_path).context("loading index")?;

    let cwd = std::env::current_dir().context("resolving cwd")?;

    // Determine the source object IDs we will need.
    // source_tree: used when restoring from a named ref (--source, or HEAD for --staged)
    // We compute it lazily to avoid resolving HEAD unnecessarily.
    let source_tree_oid: Option<ObjectId> = if let Some(ref src) = args.source {
        let oid = resolve_source(&repo, src)?;
        Some(commit_to_tree(&repo, oid)?)
    } else if restore_staged {
        // --staged without --source uses HEAD
        match resolve_source(&repo, "HEAD") {
            Ok(oid) => Some(commit_to_tree(&repo, oid)?),
            Err(_) => {
                // If there is no HEAD (empty repo), restoring staged from HEAD
                // means removing the index entries.
                None
            }
        }
    } else {
        None
    };

    // Collect all paths to operate on.
    let expanded = expand_pathspecs(
        &args.pathspec,
        &work_tree,
        &cwd,
        &index,
        source_tree_oid.as_ref(),
        &repo,
    )?;

    let mut index_modified = false;

    for rel_path in &expanded {
        let path_bytes = rel_path.as_bytes();

        // Check for unmerged (conflicted) entries in the index.
        let is_unmerged = index
            .entries
            .iter()
            .any(|e| e.path == path_bytes && e.stage() != 0);
        if is_unmerged && !args.ignore_unmerged {
            bail!(
                "path '{}' has unmerged conflicts; use --ignore-unmerged to skip",
                rel_path
            );
        }
        if is_unmerged && args.ignore_unmerged {
            continue;
        }

        if restore_staged
            && do_restore_staged(&repo, &mut index, rel_path, source_tree_oid.as_ref())?
        {
            index_modified = true;
        }

        if restore_worktree {
            // Source for worktree: --source tree (if given), else current index entry.
            if let Some(tree_oid) = &source_tree_oid {
                if !restore_staged {
                    // --source without --staged: restore worktree from tree, leave index alone
                    do_restore_worktree_from_tree(&repo, &work_tree, rel_path, *tree_oid)?;
                } else {
                    // --source with --staged (and --worktree implied or explicit)
                    do_restore_worktree_from_tree(&repo, &work_tree, rel_path, *tree_oid)?;
                }
            } else {
                // No --source: restore worktree from index
                do_restore_worktree_from_index(
                    &repo,
                    &index,
                    &work_tree,
                    rel_path,
                    args.ignore_unmerged,
                )?;
            }
        }
    }

    if index_modified {
        index.write(&index_path).context("writing index")?;
    }

    Ok(())
}

/// Restore a single path's index entry from the given tree.
///
/// Returns `true` if the index was changed.
///
/// # Errors
///
/// Returns an error if the object cannot be read or the index cannot be updated.
fn do_restore_staged(
    repo: &Repository,
    index: &mut Index,
    rel_path: &str,
    source_tree: Option<&ObjectId>,
) -> Result<bool> {
    let path_bytes = rel_path.as_bytes();

    match source_tree {
        None => {
            // No HEAD (empty repo) — remove the entry if present
            let removed = index.remove(path_bytes);
            Ok(removed)
        }
        Some(tree_oid) => {
            match find_in_tree(repo, *tree_oid, rel_path)? {
                Some((blob_oid, mode)) => {
                    // Replace / add the stage-0 entry with what HEAD has
                    let path_len = rel_path.len().min(0xFFF) as u16;
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
                        size: 0,
                        oid: blob_oid,
                        flags: path_len,
                        flags_extended: None,
                        path: path_bytes.to_vec(),
                    };
                    index.add_or_replace(entry);
                    Ok(true)
                }
                None => {
                    // Path not in source tree — remove from index
                    let removed = index.remove(path_bytes);
                    Ok(removed)
                }
            }
        }
    }
}

/// Restore a single path in the working tree from a tree object.
///
/// # Errors
///
/// Returns an error if the blob cannot be read or the file cannot be written.
fn do_restore_worktree_from_tree(
    repo: &Repository,
    work_tree: &Path,
    rel_path: &str,
    tree_oid: ObjectId,
) -> Result<()> {
    match find_in_tree(repo, tree_oid, rel_path)? {
        None => {
            bail!(
                "pathspec '{}' did not match any file(s) in the source tree",
                rel_path
            );
        }
        Some((blob_oid, mode)) => {
            let obj = repo
                .odb
                .read(&blob_oid)
                .with_context(|| format!("reading blob for '{rel_path}'"))?;
            if obj.kind != ObjectKind::Blob {
                bail!("'{}' is not a blob in the source tree", rel_path);
            }
            write_to_worktree(work_tree, rel_path, &obj.data, mode)?;
        }
    }
    Ok(())
}

/// Restore a single path in the working tree from the current index.
///
/// # Errors
///
/// Returns an error if the path is not in the index or the blob cannot be read.
fn do_restore_worktree_from_index(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    rel_path: &str,
    ignore_unmerged: bool,
) -> Result<()> {
    let path_bytes = rel_path.as_bytes();
    let entry = match index.get(path_bytes, 0) {
        Some(e) => e.clone(),
        None => {
            if ignore_unmerged {
                return Ok(());
            }
            bail!(
                "pathspec '{}' did not match any file(s) known to git",
                rel_path
            );
        }
    };

    let obj = repo
        .odb
        .read(&entry.oid)
        .with_context(|| format!("reading blob for '{rel_path}'"))?;
    if obj.kind != ObjectKind::Blob {
        bail!("'{}' is not a blob in the index", rel_path);
    }
    write_to_worktree(work_tree, rel_path, &obj.data, entry.mode)?;
    Ok(())
}

/// Write blob data to the working tree at `rel_path` under `work_tree`.
///
/// Creates parent directories as needed.  Handles symlinks and executable
/// bits based on `mode`.
///
/// # Errors
///
/// Returns an error on any filesystem failure.
fn write_to_worktree(work_tree: &Path, rel_path: &str, data: &[u8], mode: u32) -> Result<()> {
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

/// Walk a tree to find the blob (OID, mode) at `path` (slash-separated).
///
/// Returns `None` if the path does not exist in the tree.
///
/// # Errors
///
/// Returns an error if an object cannot be read or is structurally corrupt.
fn find_in_tree(
    repo: &Repository,
    tree_oid: ObjectId,
    path: &str,
) -> Result<Option<(ObjectId, u32)>> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    find_recursive(repo, tree_oid, &parts)
}

/// Recursive helper for [`find_in_tree`].
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

/// Resolve a commit/tree-ish name to an object ID.
///
/// # Errors
///
/// Returns an error if the name cannot be resolved to any object.
fn resolve_source(repo: &Repository, spec: &str) -> Result<ObjectId> {
    // Try as a full OID first
    if let Ok(oid) = spec.parse::<ObjectId>() {
        if repo.odb.exists(&oid) {
            return Ok(oid);
        }
    }

    // Try as a direct ref or DWIM
    if let Ok(oid) = refs::resolve_ref(&repo.git_dir, spec) {
        return Ok(oid);
    }
    for candidate in &[
        format!("refs/heads/{spec}"),
        format!("refs/tags/{spec}"),
        format!("refs/remotes/{spec}"),
    ] {
        if let Ok(oid) = refs::resolve_ref(&repo.git_dir, candidate) {
            return Ok(oid);
        }
    }

    bail!("ambiguous argument '{}': unknown revision", spec)
}

/// Given a commit (or tag) OID, return the root tree OID.
///
/// # Errors
///
/// Returns an error if the object is not a commit or tree, or cannot be read.
fn commit_to_tree(repo: &Repository, oid: ObjectId) -> Result<ObjectId> {
    let obj = repo.odb.read(&oid)?;
    match obj.kind {
        ObjectKind::Commit => Ok(parse_commit(&obj.data)?.tree),
        ObjectKind::Tree => Ok(oid),
        ObjectKind::Tag => {
            // Peel the tag to the underlying object
            let target_oid = peel_tag(&obj.data)?;
            commit_to_tree(repo, target_oid)
        }
        other => bail!("object {} has type {other}, expected commit or tree", oid),
    }
}

/// Extract the `object` field from a raw tag body.
fn peel_tag(data: &[u8]) -> Result<ObjectId> {
    let text =
        std::str::from_utf8(data).map_err(|_| anyhow::anyhow!("tag object is not valid UTF-8"))?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("object ") {
            return rest
                .trim()
                .parse::<ObjectId>()
                .context("invalid object ID in tag");
        }
    }
    bail!("tag object has no 'object' header")
}

/// Expand pathspecs into a list of repository-relative paths.
///
/// A pathspec of `"."` is expanded to all paths tracked in the index (for
/// index-source operations) or all paths in the source tree (for tree-source
/// operations).
///
/// # Errors
///
/// Returns an error if a path is not in the source, or on I/O failure.
fn expand_pathspecs(
    pathspecs: &[String],
    work_tree: &Path,
    cwd: &Path,
    index: &Index,
    source_tree: Option<&ObjectId>,
    repo: &Repository,
) -> Result<Vec<String>> {
    let mut result = Vec::new();

    for spec in pathspecs {
        if spec == "." {
            // Expand to all tracked paths
            if let Some(tree_oid) = source_tree {
                // Collect all paths from the source tree
                let mut tree_paths = Vec::new();
                collect_tree_paths(repo, *tree_oid, "", &mut tree_paths)?;
                result.extend(tree_paths);
            } else {
                // Collect all stage-0 paths from the index
                for entry in &index.entries {
                    if entry.stage() == 0 {
                        let path = String::from_utf8_lossy(&entry.path).into_owned();
                        result.push(path);
                    }
                }
            }
        } else {
            let rel = resolve_pathspec(spec, work_tree, cwd);
            result.push(rel);
        }
    }

    Ok(result)
}

/// Recursively collect all file paths from a tree object.
///
/// # Errors
///
/// Returns an error if any tree object cannot be read.
fn collect_tree_paths(
    repo: &Repository,
    tree_oid: ObjectId,
    prefix: &str,
    out: &mut Vec<String>,
) -> Result<()> {
    let tree_obj = repo.odb.read(&tree_oid)?;
    if tree_obj.kind != ObjectKind::Tree {
        return Ok(());
    }
    let entries = parse_tree(&tree_obj.data)?;
    for entry in entries {
        let name = String::from_utf8_lossy(&entry.name);
        let full_path = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode == 0o040000 {
            collect_tree_paths(repo, entry.oid, &full_path, out)?;
        } else {
            out.push(full_path);
        }
    }
    Ok(())
}

/// Resolve a pathspec to a repository-relative path.
///
/// Handles `"."` (returns the cwd prefix), absolute paths, and relative paths
/// from cwd within the worktree.
fn resolve_pathspec(spec: &str, work_tree: &Path, cwd: &Path) -> String {
    if spec == "." {
        return compute_prefix(work_tree, cwd).unwrap_or_default();
    }

    let candidate = PathBuf::from(spec);
    let abs = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(&candidate)
    };

    // Make relative to worktree
    if let Ok(rel) = abs.strip_prefix(work_tree) {
        rel.to_string_lossy().into_owned()
    } else {
        spec.to_owned()
    }
}

/// Compute the cwd-relative prefix inside the worktree (e.g. `"subdir"`).
fn compute_prefix(work_tree: &Path, cwd: &Path) -> Option<String> {
    let wt = work_tree.canonicalize().ok()?;
    let c = cwd.canonicalize().ok()?;
    if wt == c {
        return None;
    }
    c.strip_prefix(&wt)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}
