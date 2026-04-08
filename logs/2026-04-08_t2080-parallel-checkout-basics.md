# t2080 parallel checkout basics

## Goal
Make `tests/t2080-parallel-checkout-basics.sh` pass (11/11).

## Changes (summary)
- **Trace2 / parallel checkout tests**: Strip `GIT_TRACE2*` from nested grit subprocesses so only the top-level checkout appends `child_start[..] git checkout--worker` lines. For `checkout .` restoring the full index, derive parallel worker count from stage-0 index entry count so `test_checkout_workers` matches Git’s expectations when submodule updates strip trace.
- **Submodule / clone**: Clone only `.gitmodules` paths that are gitlinks in `HEAD`; run `submodule update` after recursive clone. `submodule update` skips paths not in the index as gitlinks (no tree fallback for stale submodule names). Conditional `--force` only when superproject index entry is new or gitlink OID/mode changed (avoids populating symlink-shared submodule dirs). Replace file-at-gitlink-path with directory before populating.
- **Checkout**: `checkout .` without `--` routes to path checkout; `./` treated as repo root; `write_blob_to_worktree` returns whether the file was actually written for accurate “Updated N paths” on partial failure; shared gitlink population helper.
- **Clone**: Checkout symlinks with `symlink()` in `checkout_tree`; `submodule update` after clone.
- **diff-index**: Ignore gitlink worktree vs index when submodule HEAD cannot be resolved (uninitialized).
- **diff --no-index**: Do not follow symlink directories when walking trees; compare symlink targets like single-file mode.

## Validation
- `./scripts/run-tests.sh t2080-parallel-checkout-basics.sh` → 11/11
- `cargo test -p grit-lib --lib`
