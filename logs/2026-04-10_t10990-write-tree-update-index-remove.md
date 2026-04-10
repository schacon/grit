# t10990 write-tree / update-index --remove

## Issue

`t10990-write-tree-clean-dirty` failed on "write-tree after update-index remove": `grit update-index --remove ui.txt` left `ui.txt` in the index while the test expected it removed (so `write-tree` would omit it).

## Root cause

`update_index` treated `--remove` as "remove only if the worktree file is missing" and refreshed the entry when the file still existed. The harness (and `t8050-update-index-modes`) expects `--remove` to drop the path from the index regardless of disk presence; `--force-remove` remains the explicit "remove even if present" in documentation, but tests align on unconditional removal for `--remove`.

## Fix

In `grit/src/commands/update_index.rs`, for `PathMode::Remove` after the skip-worktree handling, always `index.remove(&rel_bytes)` and continue (no stat-based refresh).

## Verification

- `./scripts/run-tests.sh t10990-write-tree-clean-dirty.sh` → 37/37
- `cargo test -p grit-lib --lib` → pass
