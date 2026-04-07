# t4137-apply-submodule

## Summary

Fixed `grit apply --index` / `--3way` submodule transitions to match Git.

## Changes (apply.rs)

- Parse trailing mode from `index old..new 160000` lines so gitlink patches set `old_mode`/`new_mode` and route through `apply_gitlink_to_index` (fixes `failed to read sub1: Is a directory` on submodule SHA updates).
- Run `precheck_worktree_patch_sequence` for `--index` so populated submodule work trees conflict with `sub1/file*` additions after gitlink removal (matches Git’s “already exists in working directory”).
- Skip clearing descendant existence state when deleting a gitlink in that precheck (index-only removal leaves files on disk).
- For gitlink updates, if abbreviated `new_oid` fails `resolve_revision` in the superproject ODB, fall back to full OID from `Subproject commit` hunks.

## Tests

- `tests/lib-submodule-update.sh`: `test_expect_success` for “replace submodule with a file must fail” (both variants); removes “TODO known breakage vanished” noise for t4137.

## Verification

- `sh tests/t4137-apply-submodule.sh` — all 28 tests pass.
- `cargo test -p grit-lib --lib` — pass.
