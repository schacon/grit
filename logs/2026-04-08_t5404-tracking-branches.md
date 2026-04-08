# t5404-tracking-branches

## Issue

Harness reset cwd to `TRASH_DIRECTORY` before every subtest; upstream Git preserves cwd so `cd aa` in one block affects later blocks. Test 3+ must run from clone `aa/`.

## Fixes

- `tests/test-lib-tap.sh`: stop `cd "$TRASH_DIRECTORY"` in `test_expect_success` / `test_expect_failure`.
- `grit push`: implement matching refspec `:`; skip delete refspecs when remote ref already missing; enforce `receive.denyCurrentBranch` / `receive.denyDeleteCurrent` for non-bare remotes; reject non-fast-forward per ref (continue pushing others); treat receive-pack denial as per-ref rejection.
- `grit branch`: `branch -d -r origin/foo` deletes `refs/remotes/origin/foo`.

## Verification

- `./scripts/run-tests.sh t5404-tracking-branches.sh` → 7/7
- `cargo test -p grit-lib --lib`
