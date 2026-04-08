# t13210-rev-list-count-all

## Problem

`t13210-rev-list-count-all.sh` reported 1/33 passing: after a test body ran `cd repo`, the harness shell stayed in `repo/`, so the next test’s `(cd repo && …)` tried `repo/repo` and failed.

## Fix

In `tests/test-lib-tap.sh`, after each `test_run_` in `test_expect_success` and `test_expect_failure`, `cd "$TRASH_DIRECTORY"` so every case starts from the trash root (same idea as upstream Git’s single initial `cd` into trash).

## Verification

- `./scripts/run-tests.sh t13210-rev-list-count-all.sh` → 33/33
- `./scripts/run-tests.sh t0000-basic.sh` → 92/92
- `cargo test -p grit-lib --lib` → pass
