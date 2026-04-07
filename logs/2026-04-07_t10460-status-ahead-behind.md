# t10460-status-ahead-behind

## Issue

`t10460-status-ahead-behind.sh` failed 4/5: `cd local` / `cd upstream` failed because `test_expect_success` in `tests/test-lib-tap.sh` evaluated test bodies in the shell’s current directory. After setup left cwd in `local/`, later tests could not `cd local` from there.

## Fix

Match upstream Git behavior: each `test_expect_success` body starts in `$TRASH_DIRECTORY`, sourcing `.test-exports` when present (same as `test_expect_failure`).

## Verification

- `GUST_BIN=... bash tests/t10460-status-ahead-behind.sh` — all 5 pass
- `./scripts/run-tests.sh t10460-status-ahead-behind.sh` — 5/5
