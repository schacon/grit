# t4015-diff-whitespace

## Done

- Fixed `for opt_res` / `$opts` mismatch in `tests/t4015-diff-whitespace.sh` (loop never bound `opts`, so every subtest was broken).
- `update-index --chmod`: stop chmodding the worktree; Git only updates the index (mode-only diffs vs worktree).
- `diff_index_to_worktree`: remove unsafe stat fast-path for same file size; hash worktree content always except true mode-only shortcut.
- `git diff`: `--quiet` + `--exit-code` honors differences; `-s`/`--no-patch` suppresses patch without skipping exit code; `--dirstat=lines`; trailing `--no-patch` in rev re-parse; blob-vs-file respects `--no-patch`.

## Remaining (t4015 still fails many cases)

Incomplete-line markers in unified diff, full `core.whitespace` / `--check` parity, `diff-index`/`diff-tree` check paths, color-moved, rename edge cases, etc.

## Commit

`ee6f183` — push to origin failed (remote not available in agent env).
