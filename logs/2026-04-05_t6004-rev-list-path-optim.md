# t6004-rev-list-path-optim

Date: 2026-04-05

## Reproduction

- Baseline harness: `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → **2/7**.
- Direct run (`bash tests/t6004-rev-list-path-optim.sh`) failures:
  - fail 2: `git rev-list <commit> -- .` returned `0` instead of `1`
  - fail 4: `git rev-list HEAD -- a` included merge commit unexpectedly
  - fail 5/6/7: `-- d`, `-- d/*`, and `-- d/[a-m]*` path-limited history mismatches

## Root causes

1. Path filtering in `grit-lib/src/rev_list.rs` compared only exact path keys (`commit_map.get(path)`), so:
   - `.` pathspec matched nothing
   - directory/glob pathspecs (`d`, `d/*`, `d/[a-m]*`) were effectively unsupported.
2. Merge simplification for path-limited traversal was too naive:
   - filtering by “touches path vs first parent” included merges Git omits via TREESAME simplification.

## Fixes implemented

- Updated `grit-lib/src/rev_list.rs`:
  - Reworked `commit_touches_paths(...)` to be merge-aware and parent-aware:
    - root commits: include only when any requested pathspec exists.
    - single-parent commits: include when requested paths differ from parent.
    - merge commits: detect TREESAME parents for requested pathspecs and omit merges when exactly one TREESAME parent exists (Git-like dense simplification behavior used by this suite).
  - Added `path_differs_for_specs(...)` to diff requested pathspecs across union of commit/parent trees.
  - Added `pathspec_matches(...)` with:
    - `.` / `./` support
    - literal/prefix pathspec support
    - wildcard support through `wildmatch(..., WM_PATHNAME)` for glob pathspecs.

## Validation

- Direct: `bash tests/t6004-rev-list-path-optim.sh` → **7/7** pass.
- Harness: `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → **7/7** pass.
- Regressions:
  - `./scripts/run-tests.sh t6110-rev-list-sparse.sh` → 2/2
  - `./scripts/run-tests.sh t6133-pathspec-rev-dwim.sh` → 6/6
  - `./scripts/run-tests.sh t6421-merge-partial-clone.sh` → 3/3
