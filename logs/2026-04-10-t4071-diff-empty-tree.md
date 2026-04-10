# t4071-diff-empty-tree

## Issue

Harness expected `grit diff-tree --root -r <commit>` raw / `--name-only` output to have exactly N lines (one per path). Grit emitted an extra leading commit OID line (like `git diff-tree` without `--no-commit-id`), so `test_line_count` failed for 3-file and 10-file cases.

## Fix

In `run_one_commit`, for a root commit with `--root`, skip `write_commit_header` when not using `--pretty` and not in `--stdin` mode. Stdin mode still prints the commit id first (matches Git).

## Verification

- `sh ./tests/t4071-diff-empty-tree.sh` — 32/32
- `./scripts/run-tests.sh t4071-diff-empty-tree.sh` — updates CSV + dashboards
