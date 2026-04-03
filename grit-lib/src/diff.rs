//! Diff machinery — compare trees, index entries, and working tree files.
//!
//! # Overview
//!
//! This module provides the core diffing infrastructure shared by `diff`,
//! `diff-index`, `status`, `log`, `show`, `commit`, and `merge`.
//!
//! ## Levels of comparison
//!
//! 1. **Tree-to-tree** — compare two tree objects (e.g. for `log`/`show`).
//! 2. **Tree-to-index** — compare a tree (usually HEAD) against the index
//!    (staged changes, used by `diff --cached` and `status`).
//! 3. **Index-to-worktree** — compare index against the working directory
//!    (unstaged changes, used by `diff` and `status`).
//!
//! ## Content diff
//!
//! Line-level diffing uses the Myers algorithm via the `similar` crate.
//! Output formats: unified patch, raw (`:old-mode new-mode ...`), stat,
//! numstat.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::error::{Error, Result};
use crate::index::{Index, IndexEntry};
use crate::objects::{parse_tree, ObjectId, ObjectKind, TreeEntry};
use crate::odb::Odb;

/// The kind of change between two sides of a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatus {
    /// File was added.
    Added,
    /// File was deleted.
    Deleted,
    /// File was modified (content or mode change).
    Modified,
    /// File was renamed (with optional content change).
    Renamed,
    /// File was copied.
    Copied,
    /// File type changed (e.g. regular → symlink).
    TypeChanged,
    /// Unmerged (conflict).
    Unmerged,
}

impl DiffStatus {
    /// Single-character status letter used in raw diff output.
    #[must_use]
    pub fn letter(&self) -> char {
        match self {
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Modified => 'M',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
            Self::Unmerged => 'U',
        }
    }
}

/// A single diff entry representing one changed path.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    /// The status of this change.
    pub status: DiffStatus,
    /// Path in the "old" side (None for Added).
    pub old_path: Option<String>,
    /// Path in the "new" side (None for Deleted).
    pub new_path: Option<String>,
    /// Old file mode (as octal string, e.g. "100644").
    pub old_mode: String,
    /// New file mode.
    pub new_mode: String,
    /// Old object ID (zero OID for Added).
    pub old_oid: ObjectId,
    /// New object ID (zero OID for Deleted).
    pub new_oid: ObjectId,
    /// Similarity score (0–100) for renames/copies.
    pub score: Option<u32>,
}

impl DiffEntry {
    /// The primary path for display (new_path for adds, old_path for deletes).
    #[must_use]
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or("")
    }
}

/// The zero (null) object ID used for "no object" in diff output.
pub const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// Return the zero ObjectId.
#[must_use]
pub fn zero_oid() -> ObjectId {
    ObjectId::from_bytes(&[0u8; 20]).unwrap_or_else(|_| {
        // This should never fail since we pass exactly 20 bytes
        panic!("internal error: failed to create zero OID");
    })
}

// ── Tree-to-tree diff ───────────────────────────────────────────────

/// Compare two trees and return the list of changed entries.
///
/// # Parameters
///
/// - `odb` — object database to read tree objects from.
/// - `old_tree_oid` — OID of the old tree (or `None` for comparison against empty).
/// - `new_tree_oid` — OID of the new tree (or `None` for comparison against empty).
/// - `prefix` — path prefix for nested tree recursion (empty string for root).
///
/// # Errors
///
/// Returns errors from object database reads.
pub fn diff_trees(
    odb: &Odb,
    old_tree_oid: Option<&ObjectId>,
    new_tree_oid: Option<&ObjectId>,
    prefix: &str,
) -> Result<Vec<DiffEntry>> {
    let old_entries = match old_tree_oid {
        Some(oid) => read_tree(odb, oid)?,
        None => Vec::new(),
    };
    let new_entries = match new_tree_oid {
        Some(oid) => read_tree(odb, oid)?,
        None => Vec::new(),
    };

    let mut result = Vec::new();
    diff_tree_entries(odb, &old_entries, &new_entries, prefix, &mut result)?;
    Ok(result)
}

/// Read and parse a tree object from the ODB.
fn read_tree(odb: &Odb, oid: &ObjectId) -> Result<Vec<TreeEntry>> {
    let obj = odb.read(oid)?;
    if obj.kind != ObjectKind::Tree {
        return Err(Error::CorruptObject(format!(
            "expected tree, got {}",
            obj.kind.as_str()
        )));
    }
    parse_tree(&obj.data)
}

/// Compare two sorted lists of tree entries, recursing into subtrees.
fn diff_tree_entries(
    odb: &Odb,
    old: &[TreeEntry],
    new: &[TreeEntry],
    prefix: &str,
    result: &mut Vec<DiffEntry>,
) -> Result<()> {
    let mut oi = 0;
    let mut ni = 0;

    while oi < old.len() || ni < new.len() {
        match (old.get(oi), new.get(ni)) {
            (Some(o), Some(n)) => {
                let cmp = crate::objects::tree_entry_cmp(
                    &o.name,
                    is_tree_mode(o.mode),
                    &n.name,
                    is_tree_mode(n.mode),
                );
                match cmp {
                    std::cmp::Ordering::Less => {
                        // Old entry not in new → deleted
                        emit_deleted(odb, o, prefix, result)?;
                        oi += 1;
                    }
                    std::cmp::Ordering::Greater => {
                        // New entry not in old → added
                        emit_added(odb, n, prefix, result)?;
                        ni += 1;
                    }
                    std::cmp::Ordering::Equal => {
                        // Both present — check for changes
                        if o.oid != n.oid || o.mode != n.mode {
                            let name_str = String::from_utf8_lossy(&o.name);
                            let path = format_path(prefix, &name_str);
                            if is_tree_mode(o.mode) && is_tree_mode(n.mode) {
                                // Both are trees — recurse
                                let nested = diff_trees(odb, Some(&o.oid), Some(&n.oid), &path)?;
                                result.extend(nested);
                            } else if is_tree_mode(o.mode) && !is_tree_mode(n.mode) {
                                // Tree → blob: delete tree contents, add blob
                                emit_deleted(odb, o, prefix, result)?;
                                emit_added(odb, n, prefix, result)?;
                            } else if !is_tree_mode(o.mode) && is_tree_mode(n.mode) {
                                // Blob → tree: delete blob, add tree contents
                                emit_deleted(odb, o, prefix, result)?;
                                emit_added(odb, n, prefix, result)?;
                            } else {
                                // Both blobs — modified
                                result.push(DiffEntry {
                                    status: if o.mode != n.mode && o.oid == n.oid {
                                        DiffStatus::TypeChanged
                                    } else {
                                        DiffStatus::Modified
                                    },
                                    old_path: Some(path.clone()),
                                    new_path: Some(path),
                                    old_mode: format_mode(o.mode),
                                    new_mode: format_mode(n.mode),
                                    old_oid: o.oid,
                                    new_oid: n.oid,
                    score: None,
                                });
                            }
                        }
                        oi += 1;
                        ni += 1;
                    }
                }
            }
            (Some(o), None) => {
                emit_deleted(odb, o, prefix, result)?;
                oi += 1;
            }
            (None, Some(n)) => {
                emit_added(odb, n, prefix, result)?;
                ni += 1;
            }
            (None, None) => break,
        }
    }

    Ok(())
}

fn emit_deleted(
    odb: &Odb,
    entry: &TreeEntry,
    prefix: &str,
    result: &mut Vec<DiffEntry>,
) -> Result<()> {
    let name_str = String::from_utf8_lossy(&entry.name);
    let path = format_path(prefix, &name_str);
    if is_tree_mode(entry.mode) {
        // Recurse into deleted tree
        let nested = diff_trees(odb, Some(&entry.oid), None, &path)?;
        result.extend(nested);
    } else {
        result.push(DiffEntry {
            status: DiffStatus::Deleted,
            old_path: Some(path.clone()),
            new_path: None,
            old_mode: format_mode(entry.mode),
            new_mode: "000000".to_owned(),
            old_oid: entry.oid,
            new_oid: zero_oid(),
                    score: None,
        });
    }
    Ok(())
}

fn emit_added(
    odb: &Odb,
    entry: &TreeEntry,
    prefix: &str,
    result: &mut Vec<DiffEntry>,
) -> Result<()> {
    let name_str = String::from_utf8_lossy(&entry.name);
    let path = format_path(prefix, &name_str);
    if is_tree_mode(entry.mode) {
        // Recurse into added tree
        let nested = diff_trees(odb, None, Some(&entry.oid), &path)?;
        result.extend(nested);
    } else {
        result.push(DiffEntry {
            status: DiffStatus::Added,
            old_path: None,
            new_path: Some(path),
            old_mode: "000000".to_owned(),
            new_mode: format_mode(entry.mode),
            old_oid: zero_oid(),
            new_oid: entry.oid,
                    score: None,
        });
    }
    Ok(())
}

// ── Index-to-tree diff (staged changes) ─────────────────────────────

/// Compare the index against a tree (usually HEAD's tree).
///
/// This shows "staged" changes — what would be committed.
///
/// # Parameters
///
/// - `odb` — object database.
/// - `index` — the current index.
/// - `tree_oid` — the tree to compare against (e.g. HEAD's tree), or `None`
///   for comparison against an empty tree (initial commit).
///
/// # Errors
///
/// Returns errors from ODB reads.
pub fn diff_index_to_tree(
    odb: &Odb,
    index: &Index,
    tree_oid: Option<&ObjectId>,
) -> Result<Vec<DiffEntry>> {
    // Flatten the tree into a sorted list of (path, mode, oid)
    let tree_entries = match tree_oid {
        Some(oid) => flatten_tree(odb, oid, "")?,
        None => Vec::new(),
    };

    // Build maps keyed by path
    let mut tree_map: std::collections::BTreeMap<&str, &FlatEntry> =
        std::collections::BTreeMap::new();
    for entry in &tree_entries {
        tree_map.insert(&entry.path, entry);
    }

    let mut result = Vec::new();

    // Check index entries against tree
    for ie in &index.entries {
        // Only look at stage 0 (merged) entries
        if ie.stage() != 0 {
            continue;
        }
        let path = String::from_utf8_lossy(&ie.path).to_string();
        match tree_map.remove(path.as_str()) {
            Some(te) => {
                // Present in both — check for differences
                if te.oid != ie.oid || te.mode != ie.mode {
                    result.push(DiffEntry {
                        status: DiffStatus::Modified,
                        old_path: Some(path.clone()),
                        new_path: Some(path),
                        old_mode: format_mode(te.mode),
                        new_mode: format_mode(ie.mode),
                        old_oid: te.oid,
                        new_oid: ie.oid,
                    score: None,
                    });
                }
            }
            None => {
                // In index but not tree → added
                result.push(DiffEntry {
                    status: DiffStatus::Added,
                    old_path: None,
                    new_path: Some(path),
                    old_mode: "000000".to_owned(),
                    new_mode: format_mode(ie.mode),
                    old_oid: zero_oid(),
                    new_oid: ie.oid,
                    score: None,
                });
            }
        }
    }

    // Remaining tree entries not in index → deleted
    for (path, te) in tree_map {
        result.push(DiffEntry {
            status: DiffStatus::Deleted,
            old_path: Some(path.to_owned()),
            new_path: None,
            old_mode: format_mode(te.mode),
            new_mode: "000000".to_owned(),
            old_oid: te.oid,
            new_oid: zero_oid(),
                    score: None,
        });
    }

    result.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(result)
}

// ── Index-to-worktree diff (unstaged changes) ───────────────────────

/// Compare the index against the working tree.
///
/// This shows "unstaged" changes — modifications not yet staged.
///
/// # Parameters
///
/// - `odb` — object database (for hashing worktree files).
/// - `index` — the current index.
/// - `work_tree` — path to the working tree root.
///
/// # Errors
///
/// Returns errors from I/O or hashing.
pub fn diff_index_to_worktree(
    odb: &Odb,
    index: &Index,
    work_tree: &Path,
) -> Result<Vec<DiffEntry>> {
    let mut result = Vec::new();

    for ie in &index.entries {
        if ie.stage() != 0 {
            continue;
        }
        // Use str slice directly to avoid allocation for path joining;
        // only allocate String if we need it for DiffEntry output.
        let path_str_ref = std::str::from_utf8(&ie.path).unwrap_or("");
        let file_path = work_tree.join(path_str_ref);
        match fs::symlink_metadata(&file_path) {
            Ok(meta) => {
                // Check if the file has changed using stat data first
                if stat_matches(ie, &meta) {
                    continue; // Fast path: stat data matches, assume unchanged
                }

                // Stat differs — hash the file to check actual content
                let worktree_oid = hash_worktree_file(odb, &file_path, &meta)?;
                let worktree_mode = mode_from_metadata(&meta);

                if worktree_oid != ie.oid || worktree_mode != ie.mode {
                    let path_owned = path_str_ref.to_owned();
                    result.push(DiffEntry {
                        status: DiffStatus::Modified,
                        old_path: Some(path_owned.clone()),
                        new_path: Some(path_owned),
                        old_mode: format_mode(ie.mode),
                        new_mode: format_mode(worktree_mode),
                        old_oid: ie.oid,
                        new_oid: worktree_oid,
                    score: None,
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
                || e.raw_os_error() == Some(20) /* ENOTDIR */ => {
                // File deleted from working tree (or parent replaced by a file)
                result.push(DiffEntry {
                    status: DiffStatus::Deleted,
                    old_path: Some(path_str_ref.to_owned()),
                    new_path: None,
                    old_mode: format_mode(ie.mode),
                    new_mode: "000000".to_owned(),
                    old_oid: ie.oid,
                    new_oid: zero_oid(),
                    score: None,
                });
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }

    Ok(result)
}

/// Quick stat check: does the index entry's cached stat data match the file?
pub fn stat_matches(ie: &IndexEntry, meta: &fs::Metadata) -> bool {
    // Compare size
    if meta.len() as u32 != ie.size {
        return false;
    }
    // Compare mtime (seconds + nanoseconds)
    if meta.mtime() as u32 != ie.mtime_sec {
        return false;
    }
    if meta.mtime_nsec() as u32 != ie.mtime_nsec {
        return false;
    }
    // Compare ctime (seconds + nanoseconds)
    if meta.ctime() as u32 != ie.ctime_sec {
        return false;
    }
    if meta.ctime_nsec() as u32 != ie.ctime_nsec {
        return false;
    }
    // Compare inode and device
    if meta.ino() as u32 != ie.ino {
        return false;
    }
    if meta.dev() as u32 != ie.dev {
        return false;
    }
    true
}

/// Hash a working tree file as a blob to get its OID.
fn hash_worktree_file(_odb: &Odb, path: &Path, meta: &fs::Metadata) -> Result<ObjectId> {
    let data = if meta.file_type().is_symlink() {
        // For symlinks, hash the target path
        let target = fs::read_link(path)?;
        target.to_string_lossy().into_owned().into_bytes()
    } else {
        fs::read(path)?
    };

    Ok(Odb::hash_object_data(ObjectKind::Blob, &data))
}

/// Derive a Git file mode from filesystem metadata.
fn mode_from_metadata(meta: &fs::Metadata) -> u32 {
    if meta.file_type().is_symlink() {
        0o120000
    } else if meta.mode() & 0o111 != 0 {
        0o100755
    } else {
        0o100644
    }
}

/// Compare a tree against the working tree.
///
/// Shows changes from `tree_oid` to the current working directory state.
/// Files tracked in the index but not in the tree are shown as Added.
/// Files in the tree but missing from the working tree are shown as Deleted.
///
/// # Parameters
///
/// - `odb` — object database.
/// - `tree_oid` — the tree to compare against (`None` for empty tree).
/// - `work_tree` — path to the working tree root.
/// - `index` — current index (used to discover new tracked files not in tree).
///
/// # Errors
///
/// Returns errors from ODB reads or I/O.
pub fn diff_tree_to_worktree(
    odb: &Odb,
    tree_oid: Option<&ObjectId>,
    work_tree: &Path,
    index: &Index,
) -> Result<Vec<DiffEntry>> {
    // Flatten the tree into a BTreeMap keyed by path
    let tree_flat = match tree_oid {
        Some(oid) => flatten_tree(odb, oid, "")?,
        None => Vec::new(),
    };
    let tree_map: std::collections::BTreeMap<String, &FlatEntry> =
        tree_flat.iter().map(|e| (e.path.clone(), e)).collect();

    // Build index lookup: path → &IndexEntry (stage 0 only)
    let mut index_entries: std::collections::BTreeMap<&[u8], &IndexEntry> =
        std::collections::BTreeMap::new();
    let mut index_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ie in &index.entries {
        if ie.stage() != 0 {
            continue;
        }
        let path = String::from_utf8_lossy(&ie.path).to_string();
        index_entries.insert(&ie.path, ie);
        index_paths.insert(path);
    }

    // Union of tree paths + index paths
    let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    all_paths.extend(tree_map.keys().cloned());
    all_paths.extend(index_paths.iter().cloned());

    let mut result = Vec::new();

    for path in &all_paths {
        let tree_entry = tree_map.get(path.as_str());
        let file_path = work_tree.join(path);

        let wt_meta = match fs::symlink_metadata(&file_path) {
            Ok(m) => Some(m),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(Error::Io(e)),
        };

        match (tree_entry, wt_meta) {
            (Some(te), Some(ref meta)) => {
                // Fast path: if the index entry matches the tree entry AND
                // stat cache matches, the file is unchanged — skip hashing.
                if let Some(ie) = index_entries.get(path.as_bytes()) {
                    if ie.oid == te.oid && ie.mode == te.mode && stat_matches(ie, meta) {
                        continue;
                    }
                }

                // Stat or content differs — hash the file
                let wt_oid = hash_worktree_file(odb, &file_path, meta)?;
                let wt_mode = mode_from_metadata(meta);
                if wt_oid != te.oid || wt_mode != te.mode {
                    result.push(DiffEntry {
                        status: DiffStatus::Modified,
                        old_path: Some(path.clone()),
                        new_path: Some(path.clone()),
                        old_mode: format_mode(te.mode),
                        new_mode: format_mode(wt_mode),
                        old_oid: te.oid,
                        new_oid: wt_oid,
                    score: None,
                    });
                }
            }
            (Some(te), None) => {
                // In tree but missing from worktree
                result.push(DiffEntry {
                    status: DiffStatus::Deleted,
                    old_path: Some(path.clone()),
                    new_path: None,
                    old_mode: format_mode(te.mode),
                    new_mode: "000000".to_owned(),
                    old_oid: te.oid,
                    new_oid: zero_oid(),
                    score: None,
                });
            }
            (None, Some(ref meta)) => {
                // In index but not in tree, and exists in worktree
                let wt_oid = hash_worktree_file(odb, &file_path, meta)?;
                let wt_mode = mode_from_metadata(meta);
                result.push(DiffEntry {
                    status: DiffStatus::Added,
                    old_path: None,
                    new_path: Some(path.clone()),
                    old_mode: "000000".to_owned(),
                    new_mode: format_mode(wt_mode),
                    old_oid: zero_oid(),
                    new_oid: wt_oid,
                    score: None,
                });
            }
            (None, None) => {
                // Tracked in index but neither in tree nor worktree — skip
            }
        }
    }

    result.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(result)
}

// ── Rename detection ────────────────────────────────────────────────

/// Detect renames by pairing Deleted and Added entries with similar content.
///
/// `threshold` is the minimum similarity percentage (0–100) for a pair to
/// be considered a rename (Git's default is 50%).  The function reads blob
/// content from the ODB to compute a line-level similarity score.
///
/// Exact-OID matches are always 100% similar regardless of content.
pub fn detect_renames(odb: &Odb, entries: Vec<DiffEntry>, threshold: u32) -> Vec<DiffEntry> {
    // Split entries into deleted, added, and others.
    let mut deleted: Vec<DiffEntry> = Vec::new();
    let mut added: Vec<DiffEntry> = Vec::new();
    let mut others: Vec<DiffEntry> = Vec::new();

    for entry in entries {
        match entry.status {
            DiffStatus::Deleted => deleted.push(entry),
            DiffStatus::Added => added.push(entry),
            _ => others.push(entry),
        }
    }

    if deleted.is_empty() || added.is_empty() {
        // Nothing to pair — return original order.
        let mut result = others;
        result.extend(deleted);
        result.extend(added);
        result.sort_by(|a, b| a.path().cmp(b.path()));
        return result;
    }

    // Read content for all deleted blobs.
    let deleted_contents: Vec<Option<Vec<u8>>> = deleted
        .iter()
        .map(|d| odb.read(&d.old_oid).ok().map(|obj| obj.data))
        .collect();

    // Read content for all added blobs.
    let added_contents: Vec<Option<Vec<u8>>> = added
        .iter()
        .map(|a| odb.read(&a.new_oid).ok().map(|obj| obj.data))
        .collect();

    // Build a matrix of similarity scores and find the best pairings.
    // We use a greedy approach: pick the highest-scoring pair first.
    let mut scores: Vec<(u32, usize, usize)> = Vec::new();

    for (di, del) in deleted.iter().enumerate() {
        for (ai, add) in added.iter().enumerate() {
            // Exact OID match → 100%
            if del.old_oid == add.new_oid {
                scores.push((100, di, ai));
                continue;
            }

            let score = match (&deleted_contents[di], &added_contents[ai]) {
                (Some(old_data), Some(new_data)) => {
                    compute_similarity(old_data, new_data)
                }
                _ => 0,
            };

            if score >= threshold {
                scores.push((score, di, ai));
            }
        }
    }

    // Sort: prefer same-basename pairs first, then by score descending.
    // This matches Git's behavior where basename matches are checked first.
    scores.sort_by(|a, b| {
        let a_same = same_basename(&deleted[a.1], &added[a.2]);
        let b_same = same_basename(&deleted[b.1], &added[b.2]);
        b_same.cmp(&a_same).then_with(|| b.0.cmp(&a.0))
    });

    let mut used_deleted = vec![false; deleted.len()];
    let mut used_added = vec![false; added.len()];
    let mut renames: Vec<DiffEntry> = Vec::new();

    for (score, di, ai) in &scores {
        if used_deleted[*di] || used_added[*ai] {
            continue;
        }
        used_deleted[*di] = true;
        used_added[*ai] = true;

        let del = &deleted[*di];
        let add = &added[*ai];

        renames.push(DiffEntry {
            status: DiffStatus::Renamed,
            old_path: del.old_path.clone(),
            new_path: add.new_path.clone(),
            old_mode: del.old_mode.clone(),
            new_mode: add.new_mode.clone(),
            old_oid: del.old_oid,
            new_oid: add.new_oid,
            score: Some(*score),
        });
    }

    // Collect unmatched entries.
    let mut result = others;
    result.extend(renames);
    for (i, entry) in deleted.into_iter().enumerate() {
        if !used_deleted[i] {
            result.push(entry);
        }
    }
    for (i, entry) in added.into_iter().enumerate() {
        if !used_added[i] {
            result.push(entry);
        }
    }

    result.sort_by(|a, b| a.path().cmp(b.path()));
    result
}


/// Detect copies among diff entries.
///
/// This first runs rename detection (pairing Deleted+Added), then for any
/// remaining Added entries, looks for copy sources.
///
/// - `find_copies_harder` = false: only Modified entries are copy source candidates.
/// - `find_copies_harder` = true: also examine unmodified files from `source_tree_entries`.
///
/// `source_tree_entries` should be a list of (path, mode, oid) from the source tree;
/// used when `find_copies_harder` is true to consider unmodified files as copy sources.
pub fn detect_copies(
    odb: &Odb,
    entries: Vec<DiffEntry>,
    threshold: u32,
    find_copies_harder: bool,
    source_tree_entries: &[(String, String, ObjectId)],
) -> Vec<DiffEntry> {
    // First, run rename detection to pair Delete+Add.
    let entries = detect_renames(odb, entries, threshold);

    // Split into added (remaining) and others.
    let mut added: Vec<DiffEntry> = Vec::new();
    let mut others: Vec<DiffEntry> = Vec::new();

    for entry in entries {
        match entry.status {
            DiffStatus::Added => added.push(entry),
            _ => others.push(entry),
        }
    }

    if added.is_empty() {
        return others;
    }

    // Build copy source candidates.
    let mut sources: Vec<(String, ObjectId)> = Vec::new();

    // Modified files are always candidates for -C.
    for entry in &others {
        if entry.status == DiffStatus::Modified {
            if let Some(ref old_path) = entry.old_path {
                sources.push((old_path.clone(), entry.old_oid));
            }
        }
    }

    // With find_copies_harder, also add all source tree entries.
    if find_copies_harder {
        for (path, _mode, oid) in source_tree_entries {
            if !sources.iter().any(|(p, _)| p == path) {
                sources.push((path.clone(), *oid));
            }
        }
    }

    if sources.is_empty() {
        let mut result = others;
        result.extend(added);
        result.sort_by(|a, b| a.path().cmp(b.path()));
        return result;
    }

    // Read content for sources.
    let source_contents: Vec<Option<Vec<u8>>> = sources
        .iter()
        .map(|(_, oid)| odb.read(oid).ok().map(|obj| obj.data))
        .collect();

    // Read content for added blobs.
    let added_contents: Vec<Option<Vec<u8>>> = added
        .iter()
        .map(|a| odb.read(&a.new_oid).ok().map(|obj| obj.data))
        .collect();

    // Build score matrix.
    let mut scores: Vec<(u32, usize, usize)> = Vec::new();
    for (si, (_, src_oid)) in sources.iter().enumerate() {
        for (ai, add) in added.iter().enumerate() {
            if *src_oid == add.new_oid {
                scores.push((100, si, ai));
                continue;
            }
            let score = match (&source_contents[si], &added_contents[ai]) {
                (Some(old_data), Some(new_data)) => compute_similarity(old_data, new_data),
                _ => 0,
            };
            if score >= threshold {
                scores.push((score, si, ai));
            }
        }
    }

    // Sort by score descending.
    scores.sort_by(|a, b| b.0.cmp(&a.0));

    let mut used_added = vec![false; added.len()];
    let mut copies: Vec<DiffEntry> = Vec::new();

    for (score, si, ai) in &scores {
        if used_added[*ai] {
            continue;
        }
        used_added[*ai] = true;

        let (ref src_path, _) = sources[*si];
        let add = &added[*ai];

        let src_mode = source_tree_entries
            .iter()
            .find(|(p, _, _)| p == src_path)
            .map(|(_, m, _)| m.clone())
            .unwrap_or_else(|| add.old_mode.clone());

        copies.push(DiffEntry {
            status: DiffStatus::Copied,
            old_path: Some(src_path.clone()),
            new_path: add.new_path.clone(),
            old_mode: src_mode,
            new_mode: add.new_mode.clone(),
            old_oid: sources[*si].1,
            new_oid: add.new_oid,
            score: Some(*score),
        });
    }

    let mut result = others;
    result.extend(copies);
    for (i, entry) in added.into_iter().enumerate() {
        if !used_added[i] {
            result.push(entry);
        }
    }

    result.sort_by(|a, b| a.path().cmp(b.path()));
    result
}

/// Format a rename pair using Git's compact path format.
///
/// Examples:
/// - `a/b/c` → `c/b/a` → `a/b/c => c/b/a`
/// - `c/b/a` → `c/d/e` → `c/{b/a => d/e}`
/// - `c/d/e` → `d/e` → `{c/d => d}/e`
/// - `d/e` → `d/f/e` → `d/{ => f}/e`
pub fn format_rename_path(old: &str, new: &str) -> String {
    let ob = old.as_bytes();
    let nb = new.as_bytes();

    // Find common prefix length, snapped to '/' boundary.
    let pfx = {
        let mut last_sep = 0usize;
        let min_len = ob.len().min(nb.len());
        for i in 0..min_len {
            if ob[i] != nb[i] {
                break;
            }
            if ob[i] == b'/' {
                last_sep = i + 1;
            }
        }
        last_sep
    };

    // Find common suffix length, snapped to '/' boundary.
    let mut sfx = {
        let mut last_sep = 0usize;
        let min_len = ob.len().min(nb.len());
        for i in 0..min_len {
            let oi = ob.len() - 1 - i;
            let ni = nb.len() - 1 - i;
            if ob[oi] != nb[ni] {
                break;
            }
            if ob[oi] == b'/' {
                last_sep = i + 1;
            }
        }
        last_sep
    };

    // Suffix starts at this position in each string.
    let mut sfx_at_old = ob.len() - sfx;
    let mut sfx_at_new = nb.len() - sfx;

    // If prefix and suffix overlap in both strings (both middles empty),
    // reduce the suffix so that at least the longer string has a non-empty middle.
    while pfx > sfx_at_old && pfx > sfx_at_new && sfx > 0 {
        // Reduce suffix by snapping to the next smaller '/' boundary.
        let suffix_bytes = &ob[sfx_at_old..];
        let mut new_sfx = 0;
        // Find the next '/' after sfx_at_old (i.e., reduce suffix).
        for i in 1..suffix_bytes.len() {
            if suffix_bytes[i] == b'/' {
                new_sfx = sfx - i;
                break;
            }
        }
        if new_sfx == 0 || new_sfx >= sfx {
            sfx = 0;
            sfx_at_old = ob.len();
            sfx_at_new = nb.len();
            break;
        }
        sfx = new_sfx;
        sfx_at_old = ob.len() - sfx;
        sfx_at_new = nb.len() - sfx;
    }

    // When prefix and suffix overlap in the shorter string, they share
    // the '/' boundary character. In the output format, the shared '/'
    // appears in both positions (e.g. "d/{ => f}/e" for d/e → d/f/e).
    // Compute the middle parts. When prefix and suffix overlap in a
    // string, the middle for that string is empty. The shared '/' shows
    // in both prefix (trailing) and suffix (leading) positions.
    let prefix = &old[..pfx];
    let suffix = &old[sfx_at_old..];
    let old_mid = if pfx <= sfx_at_old {
        &old[pfx..sfx_at_old]
    } else {
        ""
    };
    let new_mid = if pfx <= sfx_at_new {
        &new[pfx..sfx_at_new]
    } else {
        ""
    };

    if prefix.is_empty() && suffix.is_empty() {
        return format!("{old} => {new}");
    }

    format!("{prefix}{{{old_mid} => {new_mid}}}{suffix}")
}

/// Check if two entries share the same filename (basename).
fn same_basename(del: &DiffEntry, add: &DiffEntry) -> bool {
    let old = del.old_path.as_deref().unwrap_or("");
    let new = add.new_path.as_deref().unwrap_or("");
    let old_base = old.rsplit('/').next().unwrap_or(old);
    let new_base = new.rsplit('/').next().unwrap_or(new);
    old_base == new_base && !old_base.is_empty()
}

/// Compute a similarity percentage (0–100) between two byte slices.
///
/// Uses Git's approach: count the bytes that are "shared" (appear in
/// equal lines), then compute `score = shared_bytes * 2 * 100 / (src_size + dst_size)`.
fn compute_similarity(old: &[u8], new: &[u8]) -> u32 {
    let src_size = old.len();
    let dst_size = new.len();

    if src_size == 0 && dst_size == 0 {
        return 100;
    }
    let total = src_size + dst_size;
    if total == 0 {
        return 100;
    }

    // Use line-level diff to find shared content, then count bytes.
    use similar::{ChangeTag, TextDiff};
    let old_str = String::from_utf8_lossy(old);
    let new_str = String::from_utf8_lossy(new);
    let diff = TextDiff::from_lines(&old_str as &str, &new_str as &str);

    let mut shared_bytes = 0usize;
    for change in diff.iter_all_changes() {
        if change.tag() == ChangeTag::Equal {
            // Count bytes in the matching line (including newline).
            shared_bytes += change.value().len();
        }
    }

    // Git: score = copied * MAX_SCORE / max(src_size, dst_size)
    // We normalize to 0-100.
    let max_size = src_size.max(dst_size);
    let score = ((shared_bytes * 100) / max_size).min(100) as u32;
    score
}

// ── Output formatting ───────────────────────────────────────────────

/// Format a diff entry in Git's raw diff format.
///
/// Example: `:100644 100644 abc1234... def5678... M\tfile.txt`
pub fn format_raw(entry: &DiffEntry) -> String {
    let path = match entry.status {
        DiffStatus::Renamed | DiffStatus::Copied => {
            format!(
                "{}\t{}",
                entry.old_path.as_deref().unwrap_or(""),
                entry.new_path.as_deref().unwrap_or("")
            )
        }
        _ => entry.path().to_owned(),
    };

    let status_str = match (entry.status, entry.score) {
        (DiffStatus::Renamed, Some(s)) => format!("R{:03}", s),
        (DiffStatus::Copied, Some(s)) => format!("C{:03}", s),
        _ => entry.status.letter().to_string(),
    };

    format!(
        ":{} {} {} {} {}\t{}",
        entry.old_mode,
        entry.new_mode,
        entry.old_oid,
        entry.new_oid,
        status_str,
        path
    )
}

/// Format a diff entry with abbreviated OIDs.
pub fn format_raw_abbrev(entry: &DiffEntry, abbrev_len: usize) -> String {
    let old_hex = format!("{}", entry.old_oid);
    let new_hex = format!("{}", entry.new_oid);
    let old_abbrev = &old_hex[..abbrev_len.min(old_hex.len())];
    let new_abbrev = &new_hex[..abbrev_len.min(new_hex.len())];

    let path = entry.path();

    format!(
        ":{} {} {}... {}... {}\t{}",
        entry.old_mode,
        entry.new_mode,
        old_abbrev,
        new_abbrev,
        entry.status.letter(),
        path
    )
}

/// Generate a unified diff patch for two blobs.
///
/// # Parameters
///
/// - `old_content` — the old file content (empty for added files).
/// - `new_content` — the new file content (empty for deleted files).
/// - `old_path` — display path for the old side.
/// - `new_path` — display path for the new side.
/// - `context_lines` — number of context lines around changes (default: 3).
///
/// # Returns
///
/// The unified diff as a string.
pub fn unified_diff(
    old_content: &str,
    new_content: &str,
    old_path: &str,
    new_path: &str,
    context_lines: usize,
) -> String {
    use similar::TextDiff;

    let diff = TextDiff::from_lines(old_content, new_content);

    let mut output = String::new();
    if old_path == "/dev/null" {
        output.push_str("--- /dev/null\n");
    } else {
        output.push_str(&format!("--- a/{old_path}\n"));
    }
    if new_path == "/dev/null" {
        output.push_str("+++ /dev/null\n");
    } else {
        output.push_str(&format!("+++ b/{new_path}\n"));
    }

    let old_lines: Vec<&str> = old_content.lines().collect();

    for hunk in diff
        .unified_diff()
        .context_radius(context_lines)
        .iter_hunks()
    {
        let hunk_str = format!("{hunk}");
        // The similar crate outputs @@ -a,b +c,d @@\n but Git adds
        // function context after the closing @@. Extract the hunk header
        // and add function context.
        if let Some(first_newline) = hunk_str.find('\n') {
            let header_line = &hunk_str[..first_newline];
            let rest = &hunk_str[first_newline..];

            // Parse the old start line from the @@ header
            if let Some(func_ctx) = extract_function_context(header_line, &old_lines) {
                output.push_str(header_line);
                output.push(' ');
                output.push_str(&func_ctx);
                output.push_str(rest);
            } else {
                output.push_str(&hunk_str);
            }
        } else {
            output.push_str(&hunk_str);
        }
    }

    output
}

/// Compute a unified diff with anchored lines.
///
/// Anchored lines that appear exactly once in both old and new content are
/// forced to match, splitting the diff into segments around those anchor points.
/// This produces diffs where the anchored text stays as context and surrounding
/// lines are shown as additions/removals.
pub fn anchored_unified_diff(
    old_content: &str,
    new_content: &str,
    old_path: &str,
    new_path: &str,
    context_lines: usize,
    anchors: &[String],
) -> String {
    use similar::TextDiff;
    use std::collections::HashMap;

    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    // Find anchored lines that appear exactly once in both old and new
    let mut anchor_pairs: Vec<(usize, usize)> = Vec::new(); // (old_idx, new_idx)

    for anchor in anchors {
        let anchor_str = anchor.as_str();

        // Count occurrences in old
        let old_positions: Vec<usize> = old_lines.iter().enumerate()
            .filter(|(_, l)| l.trim_end() == anchor_str)
            .map(|(i, _)| i)
            .collect();

        // Count occurrences in new
        let new_positions: Vec<usize> = new_lines.iter().enumerate()
            .filter(|(_, l)| l.trim_end() == anchor_str)
            .map(|(i, _)| i)
            .collect();

        // Only anchor if unique in both
        if old_positions.len() == 1 && new_positions.len() == 1 {
            anchor_pairs.push((old_positions[0], new_positions[0]));
        }
    }

    // If no valid anchors, fall back to normal diff
    if anchor_pairs.is_empty() {
        return unified_diff(old_content, new_content, old_path, new_path, context_lines);
    }

    // Sort anchor pairs by their position in the old file
    anchor_pairs.sort_by_key(|&(old_idx, _)| old_idx);

    // Filter to only keep pairs where new positions are also increasing
    // (longest increasing subsequence of new positions)
    let mut filtered: Vec<(usize, usize)> = Vec::new();
    for &pair in &anchor_pairs {
        if filtered.is_empty() || pair.1 > filtered.last().unwrap().1 {
            filtered.push(pair);
        }
    }
    let anchor_pairs = filtered;

    // Build a modified version of old/new where we diff segments between anchors.
    // We'll construct the diff by processing segments:
    // - Before first anchor
    // - Between consecutive anchors
    // - After last anchor
    // Each anchor line itself is a fixed context match.

    // Collect all diff operations
    struct DiffOp {
        tag: char, // ' ', '+', '-'
        line: String,
    }

    let mut ops: Vec<DiffOp> = Vec::new();
    let mut old_pos = 0usize;
    let mut new_pos = 0usize;

    for &(old_anchor, new_anchor) in &anchor_pairs {
        // Diff the segment before this anchor
        let old_segment: Vec<&str> = old_lines[old_pos..old_anchor].to_vec();
        let new_segment: Vec<&str> = new_lines[new_pos..new_anchor].to_vec();

        let old_seg_text = old_segment.join("\n");
        let new_seg_text = new_segment.join("\n");

        if !old_seg_text.is_empty() || !new_seg_text.is_empty() {
            let old_seg_input = if old_seg_text.is_empty() { String::new() } else { format!("{}\n", old_seg_text) };
            let new_seg_input = if new_seg_text.is_empty() { String::new() } else { format!("{}\n", new_seg_text) };
            let seg_diff = TextDiff::from_lines(
                &old_seg_input,
                &new_seg_input,
            );
            for change in seg_diff.iter_all_changes() {
                let tag = match change.tag() {
                    similar::ChangeTag::Equal => ' ',
                    similar::ChangeTag::Delete => '-',
                    similar::ChangeTag::Insert => '+',
                };
                ops.push(DiffOp {
                    tag,
                    line: change.value().trim_end_matches('\n').to_string(),
                });
            }
        }

        // The anchor line itself is always context
        ops.push(DiffOp {
            tag: ' ',
            line: old_lines[old_anchor].to_string(),
        });

        old_pos = old_anchor + 1;
        new_pos = new_anchor + 1;
    }

    // Diff the remaining segment after the last anchor
    let old_segment: Vec<&str> = old_lines[old_pos..].to_vec();
    let new_segment: Vec<&str> = new_lines[new_pos..].to_vec();
    let old_seg_text = old_segment.join("\n");
    let new_seg_text = new_segment.join("\n");

    if !old_seg_text.is_empty() || !new_seg_text.is_empty() {
        let old_seg_input = if old_seg_text.is_empty() { String::new() } else { format!("{}\n", old_seg_text) };
        let new_seg_input = if new_seg_text.is_empty() { String::new() } else { format!("{}\n", new_seg_text) };
        let seg_diff = TextDiff::from_lines(
            &old_seg_input,
            &new_seg_input,
        );
        for change in seg_diff.iter_all_changes() {
            let tag = match change.tag() {
                similar::ChangeTag::Equal => ' ',
                similar::ChangeTag::Delete => '-',
                similar::ChangeTag::Insert => '+',
            };
            ops.push(DiffOp {
                tag,
                line: change.value().trim_end_matches('\n').to_string(),
            });
        }
    }

    // Now format as unified diff with hunks
    let mut output = String::new();
    if old_path == "/dev/null" {
        output.push_str("--- /dev/null\n");
    } else {
        output.push_str(&format!("--- a/{old_path}\n"));
    }
    if new_path == "/dev/null" {
        output.push_str("+++ /dev/null\n");
    } else {
        output.push_str(&format!("+++ b/{new_path}\n"));
    }

    // Group ops into hunks with context
    let total_ops = ops.len();
    if total_ops == 0 {
        return output;
    }

    // Find ranges of changes
    let mut hunks: Vec<(usize, usize)> = Vec::new(); // (start, end) indices into ops
    let mut i = 0;
    while i < total_ops {
        if ops[i].tag != ' ' {
            let start = if i > context_lines { i - context_lines } else { 0 };
            let mut end = i;
            // Extend to include consecutive changes and their context
            while end < total_ops {
                if ops[end].tag != ' ' {
                    end += 1;
                    continue;
                }
                // Check if there's another change within context_lines
                let mut next_change = end;
                while next_change < total_ops && ops[next_change].tag == ' ' {
                    next_change += 1;
                }
                if next_change < total_ops && next_change - end <= context_lines * 2 {
                    end = next_change + 1;
                } else {
                    end = (end + context_lines).min(total_ops);
                    break;
                }
            }
            // Merge with previous hunk if overlapping
            if let Some(last) = hunks.last_mut() {
                if start <= last.1 {
                    last.1 = end;
                } else {
                    hunks.push((start, end));
                }
            } else {
                hunks.push((start, end));
            }
            i = end;
        } else {
            i += 1;
        }
    }

    // Output each hunk
    for (start, end) in hunks {
        // Count old/new lines in this hunk
        let mut old_start = 1usize;
        let mut new_start = 1usize;
        // Calculate line numbers by counting ops before this hunk
        for op in &ops[..start] {
            match op.tag {
                ' ' => { old_start += 1; new_start += 1; }
                '-' => { old_start += 1; }
                '+' => { new_start += 1; }
                _ => {}
            }
        }
        let mut old_count = 0usize;
        let mut new_count = 0usize;
        for op in &ops[start..end] {
            match op.tag {
                ' ' => { old_count += 1; new_count += 1; }
                '-' => { old_count += 1; }
                '+' => { new_count += 1; }
                _ => {}
            }
        }

        output.push_str(&format!("@@ -{},{} +{},{} @@\n", old_start, old_count, new_start, new_count));
        for op in &ops[start..end] {
            output.push(op.tag);
            output.push_str(&op.line);
            output.push('\n');
        }
    }

    output
}

/// Extract function context for a hunk header.
///
/// Given a hunk header like `@@ -8,7 +8,7 @@`, find the last line
/// before line 8 in the old content that looks like a function header
/// (starts with a non-whitespace character, like Git's default).
fn extract_function_context(header: &str, old_lines: &[&str]) -> Option<String> {
    // Parse the old start line number from "@@ -<start>,<count> ..."
    let at_pos = header.find("-")?;
    let rest = &header[at_pos + 1..];
    let comma_or_space = rest.find(|c: char| c == ',' || c == ' ')?;
    let start_str = &rest[..comma_or_space];
    let start_line: usize = start_str.parse().ok()?;

    if start_line <= 1 {
        return None;
    }

    // Look backwards from the line before the hunk start for a line that
    // starts with a non-whitespace character (Git's default funcname pattern).
    // start_line is 1-indexed, so the hunk starts at old_lines[start_line-1].
    // We want to look at lines before that: old_lines[0..start_line-1].
    let search_end = (start_line - 1).min(old_lines.len());
    for i in (0..search_end).rev() {
        let line = old_lines[i];
        if !line.is_empty() {
            let first = line.as_bytes()[0];
            // Git's default: line must start with a letter, digit, '_', '$',
            // or certain other non-whitespace chars. We use a simpler heuristic:
            // any line that doesn't start with whitespace.
            if first != b' ' && first != b'\t' {
                // Truncate to 40 chars like Git does.
                let truncated = if line.len() > 40 {
                    &line[..40]
                } else {
                    line
                };
                return Some(truncated.to_owned());
            }
        }
    }
    None
}

/// Generate diff stat output (file name + insertions/deletions).
///
/// Returns a single line like: ` file.txt | 5 ++---`
pub fn format_stat_line(
    path: &str,
    insertions: usize,
    deletions: usize,
    max_path_len: usize,
) -> String {
    let total = insertions + deletions;
    let plus = "+".repeat(insertions.min(50));
    let minus = "-".repeat(deletions.min(50));
    format!(
        " {:<width$} | {:>4} {}{}",
        path,
        total,
        plus,
        minus,
        width = max_path_len
    )
}

/// Count insertions and deletions between two strings.
///
/// Returns `(insertions, deletions)`.
pub fn count_changes(old_content: &str, new_content: &str) -> (usize, usize) {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old_content, new_content);
    let mut ins = 0;
    let mut del = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => ins += 1,
            ChangeTag::Delete => del += 1,
            ChangeTag::Equal => {}
        }
    }

    (ins, del)
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Flatten a tree object recursively into a sorted list of (path, mode, oid).
struct FlatEntry {
    path: String,
    mode: u32,
    oid: ObjectId,
}

fn flatten_tree(odb: &Odb, tree_oid: &ObjectId, prefix: &str) -> Result<Vec<FlatEntry>> {
    let entries = read_tree(odb, tree_oid)?;
    let mut result = Vec::new();

    for entry in entries {
        let name_str = String::from_utf8_lossy(&entry.name);
        let path = format_path(prefix, &name_str);
        if is_tree_mode(entry.mode) {
            let nested = flatten_tree(odb, &entry.oid, &path)?;
            result.extend(nested);
        } else {
            result.push(FlatEntry {
                path,
                mode: entry.mode,
                oid: entry.oid,
            });
        }
    }

    Ok(result)
}

/// Whether a mode represents a tree (directory).
fn is_tree_mode(mode: u32) -> bool {
    mode == 0o040000
}

/// Build a path with an optional prefix.
fn format_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Format a numeric mode as a zero-padded octal string.
fn format_mode(mode: u32) -> String {
    format!("{mode:06o}")
}
