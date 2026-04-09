# t7011-skip-worktree-reading

## Goal

Make `tests/t7011-skip-worktree-reading.sh` pass 15/15 with grit.

## Changes

1. **`grit update-index`** (`grit/src/commands/update_index.rs`)
   - Match Git `process_path`: stage-0 entries with `skip-worktree` are not refreshed from disk; plain `update-index <path>` is a no-op.
   - `update-index --remove <path>` removes the entry even when the file still exists (unless `--ignore-skip-worktree-entries`).
   - When the worktree file is missing, `--remove` drops the index entry if present and does **not** error if the path was never tracked (Git `remove_one_path` / `remove_file_from_index`).

2. **`grit diff-index`** (`grit/src/commands/diff_index.rs`)
   - Skip worktree examination for index paths with `skip-worktree` (Git `do_oneway_diff` cached path).

3. **`grit commit`** (`grit/src/commands/commit.rs`)
   - Pathspec staging fails with `cannot update skip-worktree entry` when the path matches a skip-worktree index entry (tests 14–15).

4. **`grit reset`** (`grit/src/commands/reset.rs`)
   - When rebuilding the index from the target tree, preserve `assume-unchanged` and `skip-worktree` from the previous index for matching stage-0 paths so `git reset` does not clear sparse flags (Git behavior; fixes test 6 after prior tests leave the repo dirty).

## Validation

- `cargo fmt`, `cargo clippy --fix --allow-dirty -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7011-skip-worktree-reading.sh` → 15/15
