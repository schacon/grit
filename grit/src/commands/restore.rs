//! `grit restore` — restore working tree files.
//!
//! Restores specified paths in the working tree or the index from a given
//! source (index, HEAD, or an explicit tree-ish).  Unlike `reset`, this
//! command does **not** move `HEAD`.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::index::{Index, IndexEntry, MODE_EXECUTABLE, MODE_SYMLINK};
use grit_lib::objects::{parse_commit, parse_tree, ObjectId, ObjectKind};
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::io::Read;
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

    /// For unmerged paths, restore our version (stage #2) to the working tree.
    #[arg(long = "ours")]
    pub ours: bool,

    /// For unmerged paths, restore their version (stage #3) to the working tree.
    #[arg(long = "theirs")]
    pub theirs: bool,

    /// Recreate conflicted merge markers in the working tree from unmerged index entries.
    #[arg(long = "merge")]
    pub merge: bool,

    /// Conflict style (accepted for compatibility).
    #[arg(long = "conflict")]
    pub conflict: Option<String>,

    /// Interactively select hunks to discard.
    #[arg(short = 'p', long = "patch")]
    pub patch: bool,

    /// Read pathspec from file.
    #[arg(long = "pathspec-from-file")]
    pub pathspec_from_file: Option<String>,

    /// NUL-terminated pathspec input (requires --pathspec-from-file).
    #[arg(long = "pathspec-file-nul")]
    pub pathspec_file_nul: bool,

    /// Paths to restore.  Use `.` to restore all tracked files.
    #[arg()]
    pub pathspec: Vec<String>,
}

/// Run the `restore` command.
pub fn run(args: Args) -> Result<()> {
    if args.pathspec_file_nul && args.pathspec_from_file.is_none() {
        bail!("the option '--pathspec-file-nul' requires '--pathspec-from-file'");
    }
    if args.pathspec_from_file.is_some() && args.patch {
        bail!("options '--pathspec-from-file' and '--patch' cannot be used together");
    }
    if args.patch {
        bail!("not implemented: grit restore --patch");
    }
    if (args.ours || args.theirs || args.merge || args.conflict.is_some())
        && (args.staged || args.source.is_some())
    {
        bail!("these options cannot be used together");
    }

    let mut pathspecs = args.pathspec.clone();
    if let Some(ref psf) = args.pathspec_from_file {
        if !pathspecs.is_empty() {
            bail!("'--pathspec-from-file' and pathspec arguments cannot be used together");
        }
        let content = if psf == "-" {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            std::fs::read_to_string(psf)
                .with_context(|| format!("could not read pathspec from '{psf}'"))?
        };
        pathspecs = parse_pathspecs_from_file(&content, args.pathspec_file_nul)?;
    }
    if pathspecs.is_empty() {
        bail!("you must specify path(s) to restore");
    }

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
        &pathspecs,
        &work_tree,
        &cwd,
        &index,
        source_tree_oid.as_ref(),
        &repo,
    )?;

    let mut index_modified = false;

    for rel_path in &expanded {
        let path_bytes = rel_path.as_bytes();

        if args.merge && restore_worktree {
            do_restore_worktree_merge(
                &repo,
                &index,
                &work_tree,
                rel_path,
                args.conflict.as_deref(),
            )?;
            continue;
        }
        // Check for unmerged (conflicted) entries in the index.
        let is_unmerged = index
            .entries
            .iter()
            .any(|e| e.path == path_bytes && e.stage() != 0);
        if is_unmerged && args.ours && restore_worktree {
            do_restore_worktree_side(&repo, &index, &work_tree, rel_path, 2)?;
            continue;
        }
        if is_unmerged && args.theirs && restore_worktree {
            do_restore_worktree_side(&repo, &index, &work_tree, rel_path, 3)?;
            continue;
        }
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

fn do_restore_worktree_merge(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    rel_path: &str,
    conflict_style: Option<&str>,
) -> Result<()> {
    let path_bytes = rel_path.as_bytes();
    let staged_ours = index.get(path_bytes, 2).map(|e| (e.oid, e.mode));
    let staged_theirs = index.get(path_bytes, 3).map(|e| (e.oid, e.mode));
    let ((ours_oid, ours_mode), (theirs_oid, _theirs_mode)) =
        if let (Some(ours), Some(theirs)) = (staged_ours, staged_theirs) {
            (ours, theirs)
        } else if let Some(saved) = read_saved_merge_state(repo, rel_path)? {
            ((saved.0, saved.1), (saved.2, saved.3))
        } else {
            bail!("path '{}' is not unmerged", rel_path);
        };

    let ours_obj = repo
        .odb
        .read(&ours_oid)
        .with_context(|| format!("reading ours blob for '{rel_path}'"))?;
    let theirs_obj = repo
        .odb
        .read(&theirs_oid)
        .with_context(|| format!("reading theirs blob for '{rel_path}'"))?;
    if ours_obj.kind != ObjectKind::Blob || theirs_obj.kind != ObjectKind::Blob {
        bail!(
            "cannot restore merge state for non-blob path '{}'",
            rel_path
        );
    }

    let ours_text = ensure_trailing_newline(String::from_utf8_lossy(&ours_obj.data).into_owned());
    let theirs_text =
        ensure_trailing_newline(String::from_utf8_lossy(&theirs_obj.data).into_owned());

    // For now all supported styles map to standard conflict markers.
    // Accept Git's names for compatibility.
    let _ = conflict_style;
    let merged = format!(
        "<<<<<<< ours\n{}=======\n{}>>>>>>> theirs\n",
        ours_text, theirs_text
    );
    write_to_worktree(work_tree, rel_path, merged.as_bytes(), ours_mode)?;
    Ok(())
}

fn do_restore_worktree_side(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    rel_path: &str,
    stage: u8,
) -> Result<()> {
    let path_bytes = rel_path.as_bytes();
    let entry = index
        .get(path_bytes, stage)
        .ok_or_else(|| anyhow::anyhow!("path '{}' is not unmerged", rel_path))?;
    let obj = repo
        .odb
        .read(&entry.oid)
        .with_context(|| format!("reading stage {stage} blob for '{rel_path}'"))?;
    if obj.kind != ObjectKind::Blob {
        bail!("'{}' is not a blob in the index", rel_path);
    }
    write_to_worktree(work_tree, rel_path, &obj.data, entry.mode)?;
    Ok(())
}

fn ensure_trailing_newline(mut text: String) -> String {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn read_saved_merge_state(
    repo: &Repository,
    rel_path: &str,
) -> Result<Option<(ObjectId, u32, ObjectId, u32)>> {
    let state_path = repo.git_dir.join("grit-restore-merge-state");
    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    for line in content.lines() {
        let mut parts = line.splitn(5, '\t');
        let Some(path) = parts.next() else { continue };
        if path != rel_path {
            continue;
        }
        let Some(ours_oid_s) = parts.next() else {
            continue;
        };
        let Some(ours_mode_s) = parts.next() else {
            continue;
        };
        let Some(theirs_oid_s) = parts.next() else {
            continue;
        };
        let Some(theirs_mode_s) = parts.next() else {
            continue;
        };
        let ours_oid = ours_oid_s.parse::<ObjectId>()?;
        let ours_mode = u32::from_str_radix(ours_mode_s, 8).unwrap_or(0o100644);
        let theirs_oid = theirs_oid_s.parse::<ObjectId>()?;
        let theirs_mode = u32::from_str_radix(theirs_mode_s, 8).unwrap_or(0o100644);
        return Ok(Some((ours_oid, ours_mode, theirs_oid, theirs_mode)));
    }

    Ok(None)
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
    resolve_revision(repo, spec)
        .map_err(|_| anyhow::anyhow!("ambiguous argument '{}': unknown revision", spec))
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
                // Collect all tracked paths from the index, including unmerged
                // stage entries, so `restore .` can properly error/ignore on
                // conflicts depending on `--ignore-unmerged`.
                let mut seen = std::collections::BTreeSet::new();
                for entry in &index.entries {
                    let path = String::from_utf8_lossy(&entry.path).into_owned();
                    if seen.insert(path.clone()) {
                        result.push(path);
                    }
                }
            }
        } else {
            let rel = resolve_pathspec(spec, work_tree, cwd);
            if is_glob_pattern(&rel) {
                let mut matches = Vec::new();
                if let Some(tree_oid) = source_tree {
                    let mut tree_paths = Vec::new();
                    collect_tree_paths(repo, *tree_oid, "", &mut tree_paths)?;
                    for p in tree_paths {
                        if glob_matches(&rel, &p) {
                            matches.push(p);
                        }
                    }
                } else {
                    for entry in &index.entries {
                        if entry.stage() != 0 {
                            continue;
                        }
                        let p = String::from_utf8_lossy(&entry.path).into_owned();
                        if glob_matches(&rel, &p) {
                            matches.push(p);
                        }
                    }
                }
                if matches.is_empty() {
                    bail!("pathspec '{spec}' did not match any file(s) known to git");
                }
                for m in matches {
                    if !result.contains(&m) {
                        result.push(m);
                    }
                }
            } else {
                result.push(rel);
            }
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

fn parse_pathspecs_from_file(content: &str, nul_terminated: bool) -> Result<Vec<String>> {
    if nul_terminated {
        return Ok(content
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect());
    }

    let mut out = Vec::new();
    for raw in content.split_inclusive('\n') {
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if line.starts_with('"') && line.ends_with('"') && line.len() >= 2 {
            out.push(unquote_c_style(line)?);
        } else {
            out.push(line.to_string());
        }
    }
    Ok(out)
}

fn unquote_c_style(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') || bytes.len() < 2 {
        bail!("invalid C-style quoting: {s}");
    }

    let inner = &bytes[1..bytes.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' {
            out.push(inner[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= inner.len() {
            bail!("invalid escape at end of string");
        }
        match inner[i] {
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            c if c.is_ascii_digit() => {
                if i + 2 >= inner.len() {
                    bail!("truncated octal escape");
                }
                let oct = std::str::from_utf8(&inner[i..i + 3]).context("invalid octal bytes")?;
                out.push(u8::from_str_radix(oct, 8).context("invalid octal escape value")?);
                i += 2;
            }
            other => bail!("invalid escape sequence \\{}", char::from(other)),
        }
        i += 1;
    }
    String::from_utf8(out).context("invalid UTF-8 in quoted pathspec")
}

fn is_glob_pattern(spec: &str) -> bool {
    spec.contains('*') || spec.contains('?') || spec.contains('[')
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_matches_inner(pattern.as_bytes(), path.as_bytes())
}

fn glob_matches_inner(pattern: &[u8], path: &[u8]) -> bool {
    let mut pi = 0;
    let mut si = 0;
    let mut star_pi = usize::MAX;
    let mut star_si = 0;

    while si < path.len() {
        if pi < pattern.len() && pattern[pi] == b'?' && path[si] != b'/' {
            pi += 1;
            si += 1;
        } else if pi < pattern.len()
            && pattern[pi] == b'*'
            && (pi + 1 >= pattern.len() || pattern[pi + 1] != b'*')
            && !pattern[pi + 1..].contains(&b'/')
        {
            let rest = &pattern[pi + 1..];
            for i in si..=path.len() {
                if glob_matches_inner(rest, &path[i..]) {
                    return true;
                }
            }
            return false;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            if pi + 1 < pattern.len() && pattern[pi + 1] == b'*' {
                let rest = &pattern[pi + 2..];
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
            }
            if found == negate {
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
            pi = star_pi + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}
