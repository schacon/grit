# t5318-pack-objects-revs-exclude

## Symptom

Harness showed 5/9 passing. Failures were `pack-objects --revs` and `verify-pack` after the test checked out `master` again.

## Root cause

After branch switches, Git runs `pack-refs`: branch tips live only in `packed-refs`, with no loose files under `.git/refs/heads/`. `pack-objects --revs` used a local resolver that only read loose ref files, so `HEAD` → `refs/heads/master` failed with "cannot resolve ref".

## Fix

Use `grit_lib::rev_parse::resolve_revision` for non-hex stdin lines in the `--revs` path (same DWIM as `git rev-parse`). Removed the redundant `resolve_ref` helper from `pack_objects.rs`.

## Verification

- `./scripts/run-tests.sh t5318-pack-objects-revs-exclude.sh` → 9/9
- `cargo test -p grit-lib --lib`
