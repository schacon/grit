# t4058-diff-duplicates

**Date:** 2026-04-09

## Outcome

Harness reported 14/16 with 2 `test_expect_failure` blocks still marked TODO. Running the script showed both as "known breakage vanished" — grit already passes those cases.

## Changes

- `tests/t4058-diff-duplicates.sh`: `test_expect_failure` → `test_expect_success` for:
  - `can switch to another branch when status is empty`
  - `clean status, switch branches, status still clean`
- `PLAN.md` / `progress.md`: mark file complete, refresh counts.

## Verification

```bash
./scripts/run-tests.sh t4058-diff-duplicates.sh
# expect 16/16, 0 test_expect_failure in file
```

Full TAP run from `tests/` with `GUST_BIN=./grit`: all 16 ok, exit 0.
