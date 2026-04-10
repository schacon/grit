# t5616-partial-clone progress

## Changes

- **Partial clone layout (`clone.rs`)**: After `materialize_blob_none_partial_layout`, repack skeleton objects into a `pack-*.pack` + `.idx` + `.promisor` (via `pack_objects::write_partial_clone_promisor_pack`), set `core.repositoryformatversion=1` and `extensions.partialclone=<remote>`. Fixed materialization to use `Odb::write_loose_materialize` so objects are duplicated loose before packs are removed.
- **`Odb` (`odb.rs`)**: `exists_local` ignores objects only in promisor packs; `exists()` still finds them for reads. Added `write_loose_materialize`.
- **`pack_objects.rs`**: `write_partial_clone_promisor_pack` for in-process promisor pack write.
- **Checkout**: Hydrate target tree blobs from promisor before populating worktree (`promisor_hydrate::hydrate_tree_blobs_from_promisor`).
- **Fetch**: `--no-filter` flag; `copy_reachable_objects` uses `exists_local` + `write_loose_materialize` so fetches materialize blobs not only in promisor packs; inherited `blob:none` post-filter when promisor + `remote.*.partialclonefilter`; fixed `collect_blob_sets_for_tree` to count missing blob OIDs; trim marker after `--no-filter`.
- **`pull.rs`**: `no_filter: false` in fetch args struct.

## Harness

`./scripts/run-tests.sh t5616-partial-clone.sh`: early tests (1–7, 9, 11) pass; many later cases still fail (upload-pack filter protocol, refetch, HTTP, index-pack traces).

## Reason not complete

`blocked` on remaining upload-pack / filter / HTTP / maintenance semantics for the rest of t5616.
