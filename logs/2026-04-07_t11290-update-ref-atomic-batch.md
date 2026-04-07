# t11290-update-ref-atomic-batch

## Symptom

Harness reported 22/33 passing. Failures were false negatives: `test_must_fail`
rejected `test_must_fail "$GUST_BIN" ...` and `test_must_fail "$REAL_GIT" ...`
because the first argument was an absolute path (`/path/to/grit`), not the
literal `git`/`grit` command name.

## Fix

Extended `test_must_fail_acceptable` in `tests/test-lib-tap.sh` to allow
absolute paths ending in `/git`, `/grit`, or `/scalar`, matching how the harness
invokes grit and how tests invoke the real git binary.

## Verification

`./scripts/run-tests.sh t11290-update-ref-atomic-batch.sh` → 33/33 pass.
