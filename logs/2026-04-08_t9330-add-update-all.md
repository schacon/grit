# t9330-add-update-all

## Issue

`t9330-add-update-all.sh` failed at test 2+ because TAP `test_expect_success` ran bodies in the main shell; a successful `cd repo` left cwd in `repo/`, so the next test's `cd repo` failed.

Tests 14–15 failed because `git add -v` verbose lines were printed with `println!` (stdout) while the test captures stderr (`2>actual`); real Git prints these on stderr.

## Changes

1. **`tests/test-lib-tap.sh`**: Wrap `test_run_` in `( cd "$TRASH_DIRECTORY" && ... )` for both `test_expect_success` and `test_expect_failure` so each test starts from the trash root.

2. **`grit/src/commands/add.rs`**: Use `eprintln!` for verbose and dry-run `add`/`remove` progress lines (match Git and harness expectations).

## Verification

- `./scripts/run-tests.sh t9330-add-update-all.sh` — 26/26
- `./scripts/run-tests.sh t13300-add-executable-bit.sh` — pass
- `./scripts/run-tests.sh t13000-add-bulk-paths.sh` — pass
- `cargo test -p grit-lib --lib` — pass
