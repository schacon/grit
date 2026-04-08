# t4010-diff-pathspec

## Problem

`grit diff-tree` pathspec filtering used prefix-only logic (no real globs). Wildcard tests failed. `--name-only` / `--name-status` incorrectly implied `-r`, so non-recursive wildcard directory matches differed from Git.

## Changes

- Added `grit-lib::pathspec` with Git-aligned matching: default wildmatch without `WM_PATHNAME` (unless `**`), directory-prefix rule for wildcards, trailing `/` requiring directory or gitlink for exact path match.
- `diff-tree`: filter `DiffEntry` paths with mode-aware context; stop implying recursion for name-only/name-status (still implied for `--patch` / `--stat`).
- `diff-index`: reuse shared pathspec helper with index/tree mode context; removed duplicate glob matcher.
- `rev_list`: root pathspec existence check uses `matches_pathspec_with_context` with file context (blob-only map).

## Validation

- `./scripts/run-tests.sh t4010-diff-pathspec.sh` → 17/17
- `cargo test -p grit-lib --lib`, `cargo test --workspace`
- `cargo check -p grit-rs -p grit-lib`
