# t2070-restore — resolve undo (REUC)

## Problem

`git restore --worktree --merge` after resolving a conflict with `git add` or removing the path with `git rm` failed: the index no longer had stages 2/3, so Grit could not rebuild conflict markers.

## Fix

- Implemented Git’s `REUC` index extension: parse/write and in-memory `resolve_undo` map on [`Index`](grit-lib/src/index.rs).
- Record merge stages when they are dropped via [`stage_file`](grit-lib/src/index.rs), [`remove`](grit-lib/src/index.rs), and [`remove_descendants_under_path`](grit-lib/src/index.rs) (mirrors Git `record_resolve_undo` on removal).
- [`restore --merge`](grit/src/commands/restore.rs): call `unmerge_from_resolve_undo` before writing conflict markers; persist index when undo was consumed.
- Removed unused `grit-restore-merge-state` fallback.
- Unit test `resolve_undo_reuc_round_trip_and_unmerge` in `grit-lib`.

## Verification

- `./scripts/run-tests.sh t2070-restore.sh` — 15/15 pass
- `cargo test -p grit-lib --lib`
