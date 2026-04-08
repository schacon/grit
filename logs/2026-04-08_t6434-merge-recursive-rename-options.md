# t6434-merge-recursive-rename-options

## Summary

Made `tests/t6434-merge-recursive-rename-options.sh` fully pass (27/27).

## Changes

1. **`grit merge-recursive`** (`grit/src/commands/merge_recursive.rs`)
   - Parse `--find-renames`, `--find-renames=<pct>`, `--rename-threshold=<pct>`, `--no-renames` with last-wins semantics.
   - Reject negative / non-numeric similarity values (e.g. `-25`, `0xf`).
   - Truncate percentages >100 to 100 (matches Git).
   - Build `MergeRenameOptions` from config (`merge.renames` overrides `diff.renames`) then apply CLI overrides.

2. **Merge core** (`grit/src/commands/merge.rs`)
   - Added `MergeRenameOptions { detect, threshold }` and `from_config`.
   - `detect_merge_renames` takes options; skips similarity detection when disabled; uses configurable threshold.
   - `merge_trees` / `merge_trees_for_replay` take `rename_options` (merge/replay pass `from_config`; merge-recursive passes parsed CLI).

3. **`grit diff`** (`grit/src/commands/diff.rs`)
   - Fixed trailing-arg handling: `-M25%` is recognized as rename option (require digits/`%` after `-M`), not a third revision.
   - `--diff-filter=R` support for assumption test.
   - Parse `-M` / `--find-renames` percentages with optional `%` and cap at 100.

## Verification

```bash
cargo fmt
cargo clippy -p grit-rs --fix --allow-dirty
cargo test -p grit-lib --lib
./scripts/run-tests.sh t6434-merge-recursive-rename-options.sh
```
