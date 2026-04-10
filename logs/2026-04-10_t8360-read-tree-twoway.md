# t8360-read-tree-twoway

## Issue

Harness reported 20/25 passing. Failures split into:

1. **Invalid `test_must_fail grep`**: `test_must_fail` only allows git-like commands; `grep` is rejected and the test body fails before assertions.
2. **`read-tree -m -u`**: After `checkout-index` from the base tree, paths only in the target (e.g. `fileD`) exist on disk but are not in the old index. Validation treated them as untracked and aborted with "would be overwritten by merge" even when disk content already matched the target blob.

## Fix

- `tests/t8360-read-tree-twoway.sh`: use `! grep` for "must not appear in index" checks (matches upstream test-lib guidance).
- `grit/src/commands/read_tree.rs`: in `validate_worktree_updates`, for `(old_index: None, new_index: Some)` with an existing path, skip the error when `worktree_matches_entry` agrees with the new stage-0 entry.

## Verification

- `./scripts/run-tests.sh t8360-read-tree-twoway.sh` → 25/25
- `cargo test -p grit-lib --lib`
- `cargo check -p grit-rs`
