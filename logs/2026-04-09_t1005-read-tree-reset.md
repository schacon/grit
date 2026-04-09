# t1005-read-tree-reset

## Symptom

Harness showed 3/7 passing: `read-tree -u --reset` and `read-tree -u --reset HEAD HEAD` did not remove working-tree files for paths that existed only as unmerged index entries (stages 1+3, no stage 0). `reset --hard` already passed because `reset.rs` removes such paths.

## Fix

1. **`read_tree::checkout_index_entries`**: After removing stage-0 paths absent from the new index, also remove worktree paths that had only unmerged entries in the old index and no index entry at all in the new index.

2. **`checkout::checkout_index_to_worktree`**: Same unmerged-only cleanup so `checkout -f` on a branch switch path uses consistent removal logic.

3. **`force_reset_to_head`**: Load the previous index and call `checkout_index_to_worktree` before force-writing HEAD tree entries, so `checkout -f` / `checkout -f HEAD` deletes paths dropped from the index (Git parity with `force_reset_to_tree`).

## Verification

- `bash tests/t1005-read-tree-reset.sh` — 7/7
- `./scripts/run-tests.sh t1005-read-tree-reset.sh` — 7/7, CSV/dashboards updated
