# t6416-recursive-corner-cases progress (2026-04-08)

## Done

- **Virtual merge base ordering**: `create_virtual_merge_base` now folds multiple merge bases **oldest-first** (author timestamp ascending), matching Git’s `commit_list_sort_by_date` + reverse used by merge-ort. Fixes rename/add criss-cross stage-1 expectations (tests 1–7).
- **`git fast-export --no-data --all`**: Exports commits in topological+reverse order with `M`/`D`/`R`/`C` lines, marks, `from`/`merge` parents, and prefers real refs (`refs/tags/*` before `refs/heads/*`, synthetic `export-*` last).
- **`git fast-import`**: Supports `merge` parents, `R`/`C` file lines, `feature force`, and `--force` for non-fast-forward ref updates.
- **`git add`**: Clears `assume-unchanged` / `skip-worktree` when staging; removed incorrect stat-only short-circuit that skipped re-hashing when size/mtime matched (same-length one-line edits).

## Harness

- `t6416-recursive-corner-cases`: **21/40** passing (was 19/40). Remaining failures need proper **unmerged index** staging for modify/delete and other merge conflict types, plus symlink/submodule/mode handling.

## Blocked / next

- Modify/delete merge must leave `:1`/`:3` (or equivalent) in the index after conflict, not a clean stage-0 match to HEAD.
