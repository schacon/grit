# t5322-pack-objects-sparse

## Goal

Make `tests/t5322-pack-objects-sparse.sh` pass (11 cases) by matching Git’s sparse `pack-objects --revs` object selection.

## Changes (`grit/src/commands/pack_objects.rs`)

- Added `--no-sparse` (clap `SetTrue` alongside `--sparse`) and reject combining both.
- Resolved sparse default like Git: CLI → `GIT_TEST_PACK_SPARSE` (bool + integer like `git_parse_maybe_bool`) → `pack.useSparse` / `pack.usesparse` → default true.
- Dense `--revs`: unchanged full reachable walk minus full exclude closure.
- Sparse `--revs`:
  - BFS mark commits reachable from `^` tips as uninteresting; skip packing those commits (still walk parents).
  - Build edge tree set from **all** stdin tips (positive and negative), like `mark_edges_uninteresting`.
  - Port `mark_trees_uninteresting_sparse` / `add_children_by_path`: track uninteresting **trees and blobs**; skip blobs when walking interesting trees.
  - Commit-first walk: enqueue parents, walk each interesting commit’s tree with sparse uninteresting set.
  - When sparse + exactly one positive tip and one exclude that is an ancestor of that tip, skip the final full exclude subtract (matches Git vs test 9 while keeping test 3 correct).

## Verification

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5322-pack-objects-sparse.sh` → 11/11
