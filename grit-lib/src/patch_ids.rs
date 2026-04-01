//! Patch-ID computation for commit equivalence detection.
//!
//! A patch-ID is a SHA-1 digest of the normalised diff a commit introduces.
//! Whitespace is stripped from every changed line before hashing, so two
//! commits whose diffs differ only in whitespace (spaces, tabs, newlines)
//! produce identical patch-IDs.  This is the semantics required by
//! `git cherry` and `git format-patch --ignore-if-in-upstream`.
//!
//! # Algorithm
//!
//! For each file changed by the commit (sorted lexicographically by path):
//!
//! 1. Hash a header: `"diff --git a/<src> b/<dst>\n"` where both paths have
//!    all ASCII whitespace characters removed.
//! 2. For each line in the Myers diff between the old and new blob that is an
//!    addition (`+`) or deletion (`-`): hash every non-whitespace byte of the
//!    line content (the leading `+`/`-` marker is *not* included).
//!
//! Context lines (`=`) are skipped entirely.  Binary files are not supported;
//! they are treated as empty and produce only a header contribution.

use sha1::{Digest, Sha1};
use similar::{ChangeTag, TextDiff};

use crate::diff::{diff_trees, zero_oid};
use crate::error::Result;
use crate::objects::{parse_commit, ObjectId, ObjectKind};
use crate::odb::Odb;

/// Compute the patch-ID for a single commit.
///
/// Returns `None` for merge commits (more than one parent), since those do not
/// have a well-defined single-parent diff.  Root commits (no parents) are
/// compared against the empty tree.
///
/// # Parameters
///
/// - `odb` — object database used to read commit, tree, and blob objects.
/// - `commit_oid` — OID of the commit to compute the patch-ID for.
///
/// # Errors
///
/// Returns errors from object-database reads or object-parse failures.
pub fn compute_patch_id(odb: &Odb, commit_oid: &ObjectId) -> Result<Option<ObjectId>> {
    let obj = odb.read(commit_oid)?;
    if obj.kind != ObjectKind::Commit {
        return Ok(None);
    }
    let commit = parse_commit(&obj.data)?;

    // Merge commits (>1 parent) have no defined patch-id.
    if commit.parents.len() > 1 {
        return Ok(None);
    }

    // Resolve the parent tree (None = empty tree for root commits).
    let parent_tree_oid = if commit.parents.is_empty() {
        None
    } else {
        let parent_obj = odb.read(&commit.parents[0])?;
        let parent_commit = parse_commit(&parent_obj.data)?;
        Some(parent_commit.tree)
    };

    // Compute tree-to-tree diff.
    let mut diffs = diff_trees(odb, parent_tree_oid.as_ref(), Some(&commit.tree), "")?;

    // Sort by primary path (lexicographic), matching diffcore_std ordering.
    diffs.sort_by(|a, b| a.path().cmp(b.path()));

    let mut hasher = Sha1::new();

    for entry in &diffs {
        // Determine src and dst path strings (both sides always carry the
        // logical path, even for pure adds/deletes).
        let src = entry
            .old_path
            .as_deref()
            .or(entry.new_path.as_deref())
            .unwrap_or("");
        let dst = entry
            .new_path
            .as_deref()
            .or(entry.old_path.as_deref())
            .unwrap_or("");

        // Compact paths: remove all ASCII-whitespace (mirrors git's remove_space).
        let src_compact = compact_path(src);
        let dst_compact = compact_path(dst);

        // Hash the diff header line.
        let header = format!("diff --git a/{src_compact} b/{dst_compact}\n");
        hasher.update(header.as_bytes());

        // Read blob content for the old and new sides.
        let old_bytes = read_blob(odb, &entry.old_oid)?;
        let new_bytes = read_blob(odb, &entry.new_oid)?;

        // Convert to str slices for the line-diff algorithm; treat non-UTF-8
        // content as empty (binary blobs only contribute their header).
        let old_str = std::str::from_utf8(&old_bytes).unwrap_or("");
        let new_str = std::str::from_utf8(&new_bytes).unwrap_or("");

        // Hash non-whitespace bytes from every added and deleted line.
        let diff = TextDiff::from_lines(old_str, new_str);
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete | ChangeTag::Insert => {
                    let line = change.as_str().unwrap_or("");
                    for &byte in line.as_bytes() {
                        if !byte.is_ascii_whitespace() {
                            hasher.update([byte]);
                        }
                    }
                }
                ChangeTag::Equal => {}
            }
        }
    }

    let digest = hasher.finalize();
    ObjectId::from_bytes(&digest).map(Some)
}

/// Remove all ASCII-whitespace characters from a path string.
///
/// Mirrors git's `remove_space()` used when hashing diff headers.
fn compact_path(path: &str) -> String {
    path.bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| b as char)
        .collect()
}

/// Read a blob's raw bytes from the ODB.
///
/// Returns an empty `Vec` for the zero OID (representing an absent file).
fn read_blob(odb: &Odb, oid: &ObjectId) -> Result<Vec<u8>> {
    if *oid == zero_oid() {
        return Ok(Vec::new());
    }
    let obj = odb.read(oid)?;
    Ok(obj.data)
}
