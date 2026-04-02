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
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File deleted from working tree
                result.push(DiffEntry {
                    status: DiffStatus::Deleted,
                    old_path: Some(path_str_ref.to_owned()),
                    new_path: None,
                    old_mode: format_mode(ie.mode),
                    new_mode: "000000".to_owned(),
                    old_oid: ie.oid,
                    new_oid: zero_oid(),
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

    format!(
        ":{} {} {} {} {}\t{}",
        entry.old_mode,
        entry.new_mode,
        entry.old_oid,
        entry.new_oid,
        entry.status.letter(),
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

    for hunk in diff
        .unified_diff()
        .context_radius(context_lines)
        .iter_hunks()
    {
        output.push_str(&format!("{hunk}"));
    }

    output
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
