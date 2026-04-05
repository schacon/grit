# t4004-diff-rename-symlink

- Date: 2026-04-05
- Scope: Re-verify `git/t/t4004-diff-rename-symlink.sh`, update tracking files, and publish the result.

## Work log

- Read `AGENTS.md`, `PLAN.md`, and `git/t/t4004-diff-rename-symlink.sh`.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t4004 bash scripts/run-upstream-tests.sh t4004-diff-rename-symlink 2>&1 | tail -40` after clearing stale `/tmp/grit-upstream-*` scratch directories.
- Confirmed the upstream aggregate reported `Files producing TAP output: 1` and `Tests: 4 (pass: 4, fail: 0)`.
- Ran the test directly inside `/tmp/grit-upstream-workdir/t` to inspect the raw TAP output and verify all four cases passed.
- Inspected the diff/rename code paths in `grit-lib/src/diff.rs` and `grit/src/commands/diff_index.rs` to confirm the current binary already emitted the expected symlink rename/copy patch shape.
- No Rust source changes were required for this task; the remaining work item in `PLAN.md` was stale.
- Updated `PLAN.md`, `progress.md`, and `test-results.md` to reflect the verified 4/4 result.
