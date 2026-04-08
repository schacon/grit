# t12820-diff-no-index-symlink

**Date:** 2026-04-08

## Goal

Make `tests/t12820-diff-no-index-symlink.sh` fully pass under the harness.

## Result

Verified `./scripts/run-tests.sh t12820-diff-no-index-symlink.sh`: **41/41** tests pass on branch `cursor/t12820-symlink-test-passing-59b1`. No Rust changes were required; implementation already covers symlink mode `120000`, `diff`/`diff --cached`/`diff-tree`, `--stat`/`--numstat`/`--name-only`/`--name-status`, and file↔symlink type transitions.

## Follow-up

Committed dashboard refresh from the harness run and marked the task complete in `t1-plan.md`.
