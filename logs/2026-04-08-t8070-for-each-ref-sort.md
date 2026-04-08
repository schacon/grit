# t8070-for-each-ref-sort

## Symptom

Harness reported 1/30 pass: only setup succeeded; all bodies using `cd repo &&` failed.

## Root cause

`test_run_` evaluated each case in the same shell without resetting cwd. After setup, cwd stayed under `repo/`, so the next case’s `cd repo` looked for `repo/repo` and failed silently (empty `out`, wrong assertions).

## Fix

Prefix every test body in `test_run_` with `cd "$TRASH_DIRECTORY" || exit 1;` so each case starts from the trash root, matching upstream `git/t` behavior.

## Verification

- `./scripts/run-tests.sh t8070-for-each-ref-sort.sh` → 30/30
- `./scripts/run-tests.sh t13370` → 34/34 (spot-check similar pattern)
- `cargo test -p grit-lib --lib` → pass
