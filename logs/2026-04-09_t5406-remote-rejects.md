# t5406-remote-rejects

## Symptom

Harness reported 2/3: test 3 `grep rejected stderr` failed because `stderr` was empty / push ran from wrong cwd.

## Root causes

1. **Inherited `TRASH_DIRECTORY`**: When the shell exported `TRASH_DIRECTORY` (e.g. from a prior agent run), `test-lib.sh` used it as-is, so nested `lib-subtest.sh` children pointed `TRASH_DIRECTORY` at the parent trash. `setup_trash` then `rm -rf`'d paths under the subtest directory before the child could write `out`.

2. **Cwd reset between tests**: `test-lib-tap.sh` forced `cd "$TRASH_DIRECTORY"` before/after each case, and `test_eval_inner_` did the same—unlike upstream Git's test-lib, which leaves cwd across `test_expect_success` blocks. `t5406` relies on staying in `child/` after setup.

3. **Verbose blank line on fd 3**: Unconditional `echo >&3 ""` after each case wrote to stdout when not verbose (fd 3 is stdout), breaking nested subtests that capture stdout.

## Fixes

- `scripts/run-tests.sh`: `env -u TRASH_DIRECTORY -u BIN_DIRECTORY -u TEST_OUTPUT_DIRECTORY_OVERRIDE` when invoking test scripts.
- `tests/test-lib-tap.sh`: removed forced trash `cd` around cases; gate post-case blank line on `verbose=t`.
- `tests/test-lib.sh`: `test_eval_inner_` cds to trash only when `TEST_OUTPUT_DIRECTORY_OVERRIDE` is set (nested scripts from `lib-subtest.sh`).

## Verification

- `./scripts/run-tests.sh t5406-remote-rejects.sh` → 3/3
- `t0000-basic.sh` with clean env → 92/92 (sanity for subtests + verbose cases)
