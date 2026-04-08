# t1422-show-ref-exists

## Issue

`tests/show-ref-exists-tests.sh` (sourced by `t1422-show-ref-exists.sh` and `t1462-refs-exists.sh`) did not call `test_done` at the end. The harness (`scripts/run-tests.sh`) parses `# Tests: …` from TAP output emitted only in `test_done`; without it the CSV showed `timeout` and `0/0` even when all expectations passed.

## Fix

Append `test_done` to `tests/show-ref-exists-tests.sh`, matching upstream `git/t/show-ref-exists-tests.sh`.

## Verification

`./scripts/run-tests.sh t1422-show-ref-exists.sh` → ✓ 12/12.
