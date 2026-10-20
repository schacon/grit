//! Apply a clean three-way tree merge to the worktree and index with one tree diff.
//!
//! Pick, revert, and non-conflicted merges already computed [`TreeMergeOutput`]; this module
//! reuses that result for untracked guards, worktree checkout, and sparse index updates instead of
//! running separate full-tree diffs and index reloads.

use crate::diff::{diff_trees, diff_trees_with_index, DiffEntry};
use crate::error::{Error, Result};
use crate::merge_trees::{apply_tree_merge_output_to_worktree_index, TreeMergeOutput};
use crate::objects::ObjectId;
use crate::porcelain::checkout::{checkout_tree_changes_with_index_opts, CheckoutApplyOptions};
use crate::porcelain::worktree_guard::{
    collect_untracked_for_tree_changes, ensure_no_untracked_overwrite, load_worktree_snapshot,
};
use crate::repo::Repository;

/// Tree diff from `from_tree` to `merged.result_tree`, preferring index hints when `HEAD` matches.
pub(crate) fn tree_changes_for_clean_merge(
    repo: &Repository,
    from_tree: &ObjectId,
    merged: &TreeMergeOutput,
) -> Result<Vec<DiffEntry>> {
    let to_tree = merged.result_tree;
    if from_tree == &to_tree {
        return Ok(Vec::new());
    }
    let snapshot = load_worktree_snapshot(repo)?;
    let trust_index = snapshot.head_tree == Some(*from_tree)
        && !snapshot.index.entries.iter().any(|e| e.stage() != 0);
    if trust_index {
        diff_trees_with_index(
            repo.odb(),
            &snapshot.index,
            Some(from_tree),
            Some(&to_tree),
            "",
            true,
        )
    } else {
        diff_trees(repo.odb(), Some(from_tree), Some(&to_tree), "")
    }
}

/// Checkout and index update for a clean merge result (no conflict markers).
///
/// # Errors
///
/// Propagates diff, checkout, and index write failures.
pub(crate) fn apply_clean_tree_merge_to_worktree(
    repo: &Repository,
    from_tree: &ObjectId,
    merged: &TreeMergeOutput,
) -> Result<()> {
    let tree_changes = tree_changes_for_clean_merge(repo, from_tree, merged)?;
    let work_tree = repo
        .work_tree()
        .ok_or_else(|| Error::Message("this operation must be run in a work tree".into()))?;
    let mut snapshot = load_worktree_snapshot(repo)?;
    let untracked =
        collect_untracked_for_tree_changes(repo, &mut snapshot.index, work_tree, &tree_changes)?;
    ensure_no_untracked_overwrite(&untracked, &tree_changes)?;

    checkout_tree_changes_with_index_opts(
        repo,
        &tree_changes,
        &mut snapshot.index,
        CheckoutApplyOptions {
            parallel_prepare_blobs: true,
            skip_index_updates: true,
            ..CheckoutApplyOptions::default()
        },
        None,
    )?;
    apply_tree_merge_output_to_worktree_index(repo, &mut snapshot.index, merged)?;
    if snapshot.index.cache_tree.is_some() {
        snapshot.index.cache_tree_root = Some(merged.result_tree);
    }
    repo.write_index(&mut snapshot.index)?;
    Ok(())
}
