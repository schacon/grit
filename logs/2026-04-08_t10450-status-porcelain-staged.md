# t10450-status-porcelain-staged

## Symptom

Harness reported 1/35 passing; every test after the first failed even though `grit status --porcelain` behaved correctly when run manually.

## Root cause

`test_expect_success` runs each body in the same shell. The setup test ends with `cd repo`, so the next test’s `cd repo` ran from inside `repo/` and failed (no nested `repo/repo`). Upstream Git’s test-lib resets cwd to the trash directory between tests; our TAP harness did not.

## Fix

`tests/test-lib-tap.sh`: add `test_reset_cwd_to_trash` and call it before running each `test_expect_success` / `test_expect_failure` body.

## Verification

- `bash tests/t10450-status-porcelain-staged.sh` — 35/35
- `./scripts/run-tests.sh t10450-status-porcelain-staged.sh` — 35/35
