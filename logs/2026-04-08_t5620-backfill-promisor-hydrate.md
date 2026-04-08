# t5620 backfill / partial clone compliance

## Changes

- Added `grit/src/commands/promisor_hydrate.rs`: shared promisor remote resolution, blob batch fetch (local ODB + HTTP via system git), trace2 `promisor fetch_count`, marker trim, sparse tip hydration from promisor.
- `sparse-checkout set` / `add`: after applying patterns, hydrate missing tip blobs from promisor when `grit-promisor-missing` is non-empty; trim marker.
- `clone` partial `blob:none`: hydrate via promisor (not source repo path) when promisor is configured; supports HTTP clones.
- `backfill`: refactored to use shared `flush_promisor_blob_batch` / `find_promisor_source`.
- Non-cone `--sparse` walks: recurse all directories, filter blobs only (fixes `**/file.1.txt` and negated patterns); `**/` prefix matching for file paths.
- Regenerated harness CSV/dashboards: `t5620-backfill` 10/10.

## Validation

- `./scripts/run-tests.sh t5620-backfill.sh` — all 10 tests pass.
- `cargo test -p grit-lib --lib` — pass.
