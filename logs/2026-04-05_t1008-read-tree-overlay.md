# t1008-read-tree-overlay — Log

**Date:** 2026-04-05
**Branch:** main
**Status:** PASS (2/2 tests)

## Summary

`t1008-read-tree-overlay` passes after rebuilding `grit` from the current
workspace state.

## Root Cause

The test itself was not failing because of `read-tree` logic in the current
code. The local branch had a broken `grit/src/main.rs` state that prevented a
fresh `target/release/grit` build from succeeding, so the upstream runner was
not exercising a current binary.

## Changes

- rebuilt `target/release/grit`
- verified the upstream `t1008-read-tree-overlay.sh` script now passes `2/2`
- updated planning and progress tracking

## Verification

- `cargo build --release`
- `CARGO_TARGET_DIR=/tmp/grit-build-t1008 bash scripts/run-upstream-tests.sh t1008 2>&1 | tail -40`
- `CARGO_TARGET_DIR=/tmp/grit-build-t1008 cargo fmt`

Result: `t1008-read-tree-overlay` now passes `2/2`.

## Notes

- `cargo clippy --fix --allow-dirty` could not complete in this sandbox because
  Cargo failed to bind its lock-management TCP listener with
  `Operation not permitted (os error 1)`.
