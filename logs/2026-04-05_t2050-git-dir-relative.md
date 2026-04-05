# t2050-git-dir-relative — Log

**Date:** 2026-04-05
**Branch:** main
**Status:** PASS (4/4 tests)

## Summary

Re-verified `t2050-git-dir-relative` against a rebuilt `target/release/grit`
and confirmed the upstream file now passes 4/4. The remaining mismatch was in
the tracking state, not in the current Rust source tree.

## Verification

1. Read `AGENTS.md`, `PLAN.md`, and `git/t/t2050-git-dir-relative.sh`
2. Ran `CARGO_TARGET_DIR=/tmp/grit-build-t2050 bash scripts/run-upstream-tests.sh t2050 2>&1 | tail -40`
3. Rebuilt with `CARGO_TARGET_DIR=/tmp/grit-build-t2050 cargo build --release -p grit-rs`
4. Re-ran `CARGO_TARGET_DIR=/tmp/grit-build-t2050 bash scripts/run-upstream-tests.sh t2050 2>&1 | tail -40`
5. Ran `CARGO_TARGET_DIR=/tmp/grit-build-t2050 cargo fmt --all 2>/dev/null; true`
6. Confirmed `Tests: 4 (pass: 4, fail: 0)`

## Implementation Notes

- No additional Rust changes were required in the current tree
- `PLAN.md`, `progress.md`, and `test-results.md` were updated to match the
  verified upstream result

## Tooling

- `cargo fmt` completed successfully
- `cargo clippy --fix --allow-dirty` was not run for this verification-only task
