# t10000-for-each-ref-merged

## Issue

All 31/32 failures were `cd: repo: No such file or directory`. The TAP harness runs
each `test_expect_success` block in the same shell. The setup test ends with
`cd repo`, so the next test’s `cd repo` looked for `trash.../repo/repo`.

## Fix

In `tests/test-lib-tap.sh`, `cd "$TRASH_DIRECTORY"` before evaluating each
`test_expect_success` block (matches upstream assumption that cwd is trash root).

## Verification

- `bash tests/t10000-for-each-ref-merged.sh` — 32/32 pass
- `./scripts/run-tests.sh t10000-for-each-ref-merged.sh` — 32/32 pass
