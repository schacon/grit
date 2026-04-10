# t5404-tracking-branches

## Symptom

Harness reported 3/7: failures were tests 3–7 that assume the shell cwd remains in clone `aa` after the `prepare pushable branches` block.

## Cause

`tests/test-lib-tap.sh` resets to `TRASH_DIRECTORY` before each `test_expect_success` unless `TEST_LIB_INHERIT_CWD` is set. Upstream Git does not reset cwd between tests.

## Fix

Set `TEST_LIB_INHERIT_CWD=1` in `tests/t5404-tracking-branches.sh` (same pattern as `tests/t7063-status-untracked-cache.sh`).

## Verification

- `./scripts/run-tests.sh t5404-tracking-branches.sh` → 7/7
