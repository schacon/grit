# t6437-submodule-merge

## Summary

Made `tests/t6437-submodule-merge.sh` pass 22/22 with grit.

## Root cause (test 18)

`apply_directory_file_conflicts` treated a gitlink at `path` like a normal file when the other side had `path/file`, staging the **commit object** at `path~HEAD` and duplicating `path/file` in the index (second pass `(None, None, Some(te))`).

## Fixes (merge.rs)

1. Skip directory/file pre-pass when the entry at `path` is `MODE_GITLINK` (submodule vs directory is handled in the main merge pass).
2. For gitlink + theirs-has-descendants: write conflict worktree content from the **directory file blob** (`te.oid`), not the gitlink commit.
3. Mark all `theirs_entries` keys under `path/` as `handled_paths` so `path/file` is not merged twice.
4. `remove_deleted_files`: when old gitlink at `path` is gone from index but unmerged gitlink lives at `path~…`, do not delete the submodule checkout at `path/`.
5. `checkout_entries`: skip writing stage-0 blobs whose path is under a nested `.git` directory (keep submodule work tree clean).

## Other files (from branch / prior work)

Submodule ODB alternates, `read_submodule_head_oid` ref resolution, `EISDIR` handling in refs/diff-index, rev-parse gitlink paths, submodule merge logic, etc.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t6437-submodule-merge.sh` → 22/22
