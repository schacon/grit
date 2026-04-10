# t6416-recursive-corner-cases

## Progress

- **fast-export**: Include peeled `refs/tags/*` in revision sources so `fast-export --all` + `fast-import` after `git tag E` does not fail with `no ref source for commit` (test 7).
- **commit**: Allow merge commits when index tree matches first parent (`parents.len() > 1` skips "nothing to commit") so modify/delete resolution can conclude (tests 8–10).
- **merge / virtual base**: When folding merge bases, synthesize modify/delete virtual blobs from **base** (not modified side); special-case directory/file 1+2 / 1+3 conflicts to keep **ours/theirs file** in the virtual tree; inner `merge_trees` during virtual-base construction uses merge-ort-style directory/file handling + `criss_cross_outer` flag for outer recursive merges.
- **rename/delete**: When they deleted source and did not add at destination, stage `:1` + `:2` at destination without duplicate modify/delete message.
- **Harness**: t6416 now **23/40** (was 19/40).

## Remaining

Tests 12+ still fail: merge-ort parity for rename/delete with virtual merge bases (`Temporary merge branch` paths), symlinks, submodules, mode conflicts, nested conflict markers. Upstream Git 2.43 on the same graph for the D1/E1 merge shows only `:2:a` (no `:1:a`), while the ported test expects three index lines including `:1:a` — may need merge-ort alignment or test expectation refresh against current Git.

## Blocked

None; further work is substantial merge-ort feature parity.
