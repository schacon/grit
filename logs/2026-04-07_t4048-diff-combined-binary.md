# t4048-diff-combined-binary

## Summary

Made `tests/t4048-diff-combined-binary.sh` pass 14/14.

## Changes

- **grit-lib `crlf`**: Replaced `FileAttrs.diff_driver` with `DiffAttr` enum so `-diff` / `diff=unset` is distinct from `diff=<driver>` (needed for `text -diff` binary-style diffs).
- **grit-lib `merge_diff`**: New module for combined diff paths, textconv (`diff.*.textconv`, strip trailing shell `<` when stdin is wired), binary via NUL + `-diff`, two-parent combined hunk formatting, worktree conflict `diff --cc` output.
- **`grit show`**: `-c` / `--cc`; merge commits default to combined diff; `-m` with `--format=%s` repeats subject between parent diffs; textconv + binary for normal and merge paths.
- **`grit diff-tree`**: Parse `-c` / `--cc`; merge commit + patch uses combined diff without textconv (plumbing).
- **`grit diff`**: With `MERGE_HEAD` and unmerged index, emit combined conflict diff for stage 1/2/3; filter duplicate Modified after Unmerged; textconv on conflict sides.

## Validation

- `./scripts/run-tests.sh t4048-diff-combined-binary.sh` → 14/14
- `cargo test -p grit-lib --lib`
