# t7409-submodule-detached-work-tree

## Summary

Made `tests/t7409-submodule-detached-work-tree.sh` pass (3/3) under `./scripts/run-tests.sh`.

## Root causes

1. **Merge/pull** refused updates because `is_worktree_entry_dirty` used `fs::read` on gitlink paths (directories), always treating submodules as dirty.
2. **Checkout** after `clone --bare` + `checkout main` did not populate the work tree when HEAD already pointed at `main` with an empty index (`switch_branch` early return).
3. **Submodule update** ran `checkout <oid>` in detached mode but `detach_head_inner` skipped `switch_to_tree` when OID matched symbolic HEAD, leaving `--no-checkout` clones without an index or files.
4. **`clone --separate-git-dir`** for submodules did not set `core.worktree` in the module git-dir (Git does), so checkout had nowhere to write.
5. **Nested grit** in `submodule update` inherited parent `GIT_DIR`/`GIT_WORK_TREE`, breaking `clone` into the submodule path.
6. **Harness** could inherit stray `GIT_DIR` from the agent environment; `run-tests.sh` now runs tests with `env -u GIT_DIR -u GIT_WORK_TREE`.

## Files touched

- `grit/src/commands/merge.rs` — gitlink dirty detection via submodule HEAD OID
- `grit/src/commands/checkout.rs` — same for `is_worktree_dirty`; empty-index branch checkout; detach when index empty
- `grit/src/commands/submodule.rs` — `grit_subprocess`, `set_separate_gitdir_worktree`, wire into clone/update/attach
- `scripts/run-tests.sh` — clear `GIT_DIR`/`GIT_WORK_TREE` for each test file

## Validation

- `./scripts/run-tests.sh t7409-submodule-detached-work-tree.sh` → 3/3
- `cargo test -p grit-lib --lib` → pass
