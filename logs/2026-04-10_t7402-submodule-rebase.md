# t7402-submodule-rebase

## Summary

Made `tests/t7402-submodule-rebase.sh` pass (6/6).

## Changes

- **`rev_parse`**: `treeish:path` resolution now returns gitlink (and tree) OIDs for `rev-parse HEAD:submodule` instead of rejecting gitlinks as “not a blob”.
- **`diff`**: Added `ignore_submodules` flag to `diff_index_to_tree` / `diff_index_to_worktree`; rebase pre-flight uses `true` to match Git `require_clean_work_tree(..., ignore_submodules=1)`.
- **`rebase`**: Do not `remove_dir_all` populated submodule worktrees when writing gitlink entries; print upstream-style submodule merge conflict advice when cherry-pick stops on gitlink conflicts.
- **`stash`**: Preserve submodule `.git` on reset; record submodule HEAD in stash worktree tree for gitlink paths; apply path skips gitlinks and avoids reading dirs as files; conflict pre-check skips gitlinks.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7402-submodule-rebase.sh`
