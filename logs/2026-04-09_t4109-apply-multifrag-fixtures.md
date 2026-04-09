# t4109-apply-multifrag

## Issue

Harness reported 0/3: `tests/t4109-apply-multifrag.sh` copies
`$TEST_DIRECTORY/t4109/patch*.patch`, but only `expect-*` lived under
`tests/t4109/`; patch files were missing (they exist upstream in `git/t/t4109/`).

## Fix

Copied `patch1.patch` … `patch4.patch` from `git/t/t4109/` into `tests/t4109/`.

## Verification

- `./scripts/run-tests.sh t4109-apply-multifrag.sh` → 3/3
- `cargo test -p grit-lib --lib` → pass
