# t9750-merge-base-octopus

## Symptom

`./scripts/run-tests.sh t9750-merge-base-octopus.sh` reported 32/35 failures. Verbose run (`-v -i`) showed the real error: `cd: linear: No such file or directory` on test 2.

## Root cause

`tests/test-lib-tap.sh` runs each `test_expect_success` body in the **same shell** as `setup_trash`. Test 1 ends with `cd linear`, so the cwd stays `trash.../linear`. Test 2 runs `cd linear` again (relative to trash), which fails. Earlier analysis mistook this for merge-base or `$(...)` / stdout issues.

## Fix

Before `test_run_` in both `test_expect_success` and `test_expect_failure`, `cd "$TRASH_DIRECTORY"` so every test starts from the trash root, matching upstream Git harness behavior.

## Verification

- `./scripts/run-tests.sh t9750-merge-base-octopus.sh` → 35/35 pass
- `cargo test -p grit-lib --lib` → pass
