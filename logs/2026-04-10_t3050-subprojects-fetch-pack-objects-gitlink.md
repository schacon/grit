# t3050-subprojects-fetch — pack-objects gitlink fix

## Problem

`git pull` in clone failed with `missing object in non-promisor repository` from `pack-objects` during `upload-pack`. Superproject history had a tree with mode `160000` (submodule/gitlink) whose OID is a commit in the nested repo, not an object in the superproject ODB.

## Fix

In `grit/src/commands/pack_objects.rs`, `walk_reachable` now parses trees with `parse_tree` and skips entries with mode `0o160000`, matching Git’s behavior of not walking into submodule commits when packing the superproject.

## Validation

- `sh tests/t3050-subprojects-fetch.sh` — 4/4
- `./scripts/run-tests.sh t3050-subprojects-fetch.sh` — 4/4
- `cargo test -p grit-lib --lib` — pass
