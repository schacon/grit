# t6422 merge rename corner cases

## Changes

- `rev-parse`: resolve bare paths via index DWIM (`resolve_revision`) so `git rev-parse b` after merge matches Git when `b` is only in the index.
- `merge`: skip Case 1 rename handling when the other side has tree entries under the rename destination (directory/file under rename target); three-way merge for D/F when we renamed into the conflict path; stage content conflicts at `path~HEAD` with merge-file-compatible labels; `remove_deleted_files` removes a file when the merged index needs a directory underneath.
- `ls-files -o`: on Linux, when stdout is redirected and multiple untracked paths exist, omit the redirect target path (t6422 `ls-files -o >out` line counts).

## Tests

- `tests/t6422-merge-rename-corner-cases.sh`: synced from `git/t/`; flipped `conflict caused if rename not detected` to `test_expect_success` (passes with grit).
- Harness: `./scripts/run-tests.sh t6422-merge-rename-corner-cases.sh` → 11/26 passing (remaining upstream `test_expect_failure` / merge gaps).
