# t9300-branch-delete-force: harness cwd fix

## Problem

`t9300-branch-delete-force.sh` failed 21/25: `cd repo` in later tests could not find `repo` because the first setup test left the shell cwd inside `repo/`, so relative `cd repo` resolved to `repo/repo`.

## Fix

In `tests/test-lib.sh`, `test_run_` now prefixes each test body and cleanup with `cd "$TRASH_DIRECTORY"` so every test starts from the trash root (same invariant as upstream Git’s test-lib after `cd -P "$TRASH_DIRECTORY"`).

## Verification

- `./scripts/run-tests.sh t9300-branch-delete-force.sh` → 25/25
- `cargo fmt`, `cargo check -p grit-rs`, `cargo clippy -p grit-rs --fix --allow-dirty`
- `cargo test -p grit-lib --lib` → 121 passed (pre-existing warnings in odb.rs tests)
