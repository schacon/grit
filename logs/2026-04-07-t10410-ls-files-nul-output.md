# t10410-ls-files-nul-output

## Issue

`t10410-ls-files-nul-output.sh` failed after the first test: `cd: repo: No such file or directory`.

## Root cause

`test_expect_success` in `tests/test-lib-tap.sh` evaluated each test block in the **same shell** as the previous test. Test 1 left cwd inside `repo/`, so test 2’s `cd repo` tried `repo/repo`.

Upstream Git’s test library resets cwd to the trash directory before each test block.

## Fix

In `_test_eval_inner`, `cd "$TRASH_DIRECTORY"` before `eval` so every `test_expect_success` starts from the trash root.

## Verification

- `./scripts/run-tests.sh t10410-ls-files-nul-output.sh` — 31/31 pass.
- `cargo test -p grit-lib --lib` — pass.
