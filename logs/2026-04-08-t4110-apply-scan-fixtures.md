# t4110-apply-scan

## Issue

`t4110-apply-scan.sh` failed because it looks for patches at
`$TEST_DIRECTORY/t4110/patch*.patch`, but only `expect` lived at `tests/t4110/`;
the five patch files were only under the nested `tests/t4110/t4110/` directory
(copied incorrectly from upstream layout).

## Fix

Copied `patch1.patch` … `patch5.patch` from `git/t/t4110/` into `tests/t4110/`
so paths match the test script. Grit `apply` already handled the scan case;
no Rust changes.

## Verification

- `./scripts/run-tests.sh t4110-apply-scan.sh` → 1/1 pass
- `cargo test -p grit-lib --lib` → pass
