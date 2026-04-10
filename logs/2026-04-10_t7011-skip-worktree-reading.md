# t7011-skip-worktree-reading (2026-04-10)

## Goal

Make `tests/t7011-skip-worktree-reading.sh` pass 15/15.

## Failures (before)

- `diff-index` raw lines for skip-worktree staged adds showed all-zero new OID; Git shows index blob OID (`EMPTY_BLOB`).
- Dirty skip-worktree: first `diff_tree_vs_worktree` pass overwrote `new` with zero OID from disk.
- `git commit <path>` with skip-worktree: `stage_pathspecs_for_commit` staged from disk without rejecting skip-worktree.

## Changes

- `grit/src/commands/diff_index.rs`: skip refreshing `RawChange.new` from worktree when index entry has skip-worktree or assume-unchanged. Raw output: uncached `A` still uses zero/zero placeholder (t1501) except when index has skip-worktree or assume-unchanged (t7011).
- `grit/src/commands/add.rs`: `stage_pathspecs_for_commit` rejects skip-worktree paths (aligned with `commit` dry-run path).

## Verification

- `./scripts/run-tests.sh t7011-skip-worktree-reading.sh` → 15/15
- `t1501-work-tree.sh` diff-index case still passes (23 fails pre-existing: `_gently`).
