# t9030-commit-tree-parents (2026-04-09)

## Symptom

All 25 tests in `tests/t9030-commit-tree-parents.sh` failed after test 1. `commit-tree`, `cat-file`, and `rev-parse` behaved correctly when run manually in the same repo.

## Root cause

The first test ends with `cd repo` inside the test body. The harness did not reset the working directory between tests, so the next test ran `cd repo` from **inside** `repo/`, resolving to `repo/repo` (missing). Every subsequent command failed.

## Fix

In `tests/test-lib-tap.sh`, `cd "$TRASH_DIRECTORY"` before `test_run_` in both `test_expect_success` and `test_expect_failure`, matching upstream Git’s behavior (each test starts from the trash root).

## Verification

- `./scripts/run-tests.sh t9030-commit-tree-parents.sh` — 25/25 pass
- `./scripts/run-tests.sh t0000-basic.sh` — 92/92 pass (sanity)
