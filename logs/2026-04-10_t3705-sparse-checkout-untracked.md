# t3705-add-sparse-checkout — untracked file cleanup

## Problem

After test 16 (`git sparse-checkout set a`), untracked fixture files from test 1
(`sparse_error_header`, `sparse_hint`, `sparse_entry_error`) were deleted from
the trash directory. Tests 17 and 20 then failed: missing expected files or
wrong `stderr` (hints when `advice.updateSparsePath=false`).

## Root cause

`apply_sparse_patterns` called `remove_untracked_outside_sparse`, which walked
the work tree and removed any untracked path outside the sparse definition.
Upstream Git’s `sparse-checkout set` does not do this broad untracked sweep;
it updates the index and working tree via `update_sparsity` / unpack-trees
semantics without deleting arbitrary untracked repo-root files.

## Fix

Removed the `remove_untracked_outside_sparse` pass (and helper) from
`grit/src/commands/sparse_checkout.rs` so changing sparse patterns no longer
deletes untracked files.

## Verification

- `sh ./tests/t3705-add-sparse-checkout.sh` — 20/20 pass
- `./scripts/run-tests.sh t3705-add-sparse-checkout.sh` — updates CSV/dashboards
