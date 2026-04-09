# diff-index uncached: tree vs working tree when index differs

## Problem

`t9231-diff-index-patch` failed on `diff-index -p HEAD` after staging changes then modifying the work tree again. Grit emitted `diff --git` with no `---`/`+++` hunk because the diff compared HEAD to the **index** blob only.

## Fix

In `diff_tree_vs_worktree`, after merging tree↔index changes, adjust `A`/`M` entries when the index differs from the tree: if the work tree still matches the index (stat cache or content hash), keep the index snapshot as the "new" side; otherwise replace the new side with a work-tree placeholder (zero OID, mode from disk) so `-p` reads file content from the work tree, matching Git.

## Validation

- `./scripts/run-tests.sh t9231-diff-index-patch.sh` → 11/11
- `sh tests/t1501-work-tree.sh` → 39/39 (sanity)
- `cargo test -p grit-lib --lib` → pass
