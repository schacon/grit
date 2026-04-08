# t0095-bloom — EXPENSIVE prerequisite

## Issue

`t0095-bloom.sh` test 11 (`get bloom filter for commit with 513 changes`) is tagged `EXPENSIVE`. Upstream Git defines that prerequisite as “enabled when `GIT_TEST_LONG` is set”. Our simplified `tests/test-lib.sh` never registered `EXPENSIVE`, so `test_have_prereq` always failed and the subtest was skipped as “missing EXPENSIVE” even with `GIT_TEST_LONG=1`.

## Change

Added `test_lazy_prereq EXPENSIVE` matching `git/t/test-lib.sh` so expensive subtests can run when `GIT_TEST_LONG` is exported.

## Verification

- `GIT_TEST_LONG=1 bash tests/t0095-bloom.sh` — all 11 tests pass (11th runs, does not skip).
- `./scripts/run-tests.sh t0095-bloom.sh` — still reports ok (default run skips expensive case by design).
