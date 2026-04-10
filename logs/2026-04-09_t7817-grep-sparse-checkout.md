# t7817-grep-sparse-checkout

## Summary

Made `tests/t7817-grep-sparse-checkout.sh` pass 8/8.

## Key fixes

- **Non-cone sparse matching** (`grit-lib/src/ignore.rs`): parse `/*` / `!/*/` like Git; parent-walk stops at top level without false include; optional work-tree dtype for directory-only patterns; anchored `/*` matches single segment only; `!/*/` directory pass matches nested paths.
- **`path_matches_sparse_patterns`** (`grit-lib/src/sparse_checkout.rs`): `inner == "*"` for `/*/` lines; skip clearing skip-worktree when `assume_unchanged` (grep work-tree semantics).
- **`apply_sparse_checkout_skip_worktree`**: takes optional work tree; threaded through read-tree/checkout/merge/rm.
- **`sparse-checkout disable`**: apply in-memory `/*` only — do not overwrite stored pattern file with lone `/*` (t7817 merge flow).
- **Submodule cleanup**: skip `remove_untracked_outside_sparse` inside gitlink dirs; reapply submodule sparse after super apply.
- **`update-index`**: bit flags before skip-worktree early-continue; promote index to v3 when setting skip-worktree.
- **`merge --abort`**: remove work-tree files for restored `skip-worktree` entries so conflict markers do not block `checkout main` (root cause of tests 4/6/8 failing after test 3).
- **`grep`**: `clear_skip_worktree_from_present_files` before work-tree walk; skip both bits for work-tree path; CE_VALID+skip-worktree early continue.

## Validation

`./scripts/run-tests.sh t7817-grep-sparse-checkout.sh` → 8/8
