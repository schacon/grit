# t2050-git-dir-relative — Log

**Date:** 2026-04-05
**Branch:** main
**Status:** PASS (4/4 tests)

## Summary

Fixed repository discovery for explicit relative `GIT_DIR` so non-bare repos
default the work tree to the caller's current directory when `GIT_WORK_TREE`
is unset. This restores `git add`/`git commit` behavior and post-commit hook
execution after moving `.git` below the work tree.

## Verification

1. Read `AGENTS.md`, `PLAN.md`, and `git/t/t2050-git-dir-relative.sh`
2. Ran `CARGO_TARGET_DIR=/tmp/grit-build-t2050 bash scripts/run-upstream-tests.sh t2050 2>&1 | tail -40`
3. Rebuilt with `cargo build --release`
4. Added a regression test in `grit-lib/src/repo.rs`
5. Re-ran `CARGO_TARGET_DIR=/tmp/grit-build-t2050 bash scripts/run-upstream-tests.sh t2050 2>&1 | tail -40`
6. Confirmed `Tests: 4 (pass: 4, fail: 0)`

## Implementation Notes

- `grit-lib/src/repo.rs` now infers the work tree from `cwd` for explicit
  non-bare `GIT_DIR` usage unless the caller is already inside the git dir
- Added a focused regression test for the explicit relative `GIT_DIR` case

## Tooling

- `cargo fmt` completed successfully
- `cargo clippy --fix --allow-dirty` could not run in this sandbox because
  Cargo failed before compilation with `failed to bind TCP listener to manage locking`
