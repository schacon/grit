# t1011-read-tree-sparse-checkout

## Summary

Made `tests/t1011-read-tree-sparse-checkout.sh` pass (23/23).

## Changes

- **`grit-lib` `sparse_checkout`**: `apply_sparse_checkout_skip_worktree` now takes `skip_sparse_checkout`; uses Git-style non-cone rules via `ignore::path_in_sparse_checkout`; cone mode uses silent `ConePatterns::try_parse` (no spam on raw patterns); empty on-disk sparse file with no patterns excludes all paths.
- **`read-tree`**: `--no-sparse-checkout`; apply sparse after merge; validate worktree before clearing skip-worktree for dirty paths; skip untracked-overwrite check for new skip-worktree entries; `checkout_index_entries` skips checkout when disk differs from index for materialized paths; `--reset` applies sparse before checkout.
- **`checkout`**: bare `git checkout` with sparse enabled calls `switch_to_tree` (re-apply patterns); `warn_sparse_paths_already_present` for materializing paths with existing files; `checkout .` removes skip-worktree paths from disk then restores; `--ignore-skip-worktree-bits` honored in path checkout.
- **`merge` / `merge_resolve`**: pass new `apply_sparse_checkout_skip_worktree` flag.

## Validation

- `./scripts/run-tests.sh t1011-read-tree-sparse-checkout.sh` → 23/23
- `cargo fmt`, `cargo clippy --fix --allow-dirty`, `cargo test -p grit-lib --lib`
