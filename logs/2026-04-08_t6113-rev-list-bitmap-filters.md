# t6113-rev-list-bitmap-filters

## Result

All 14 tests pass via `./scripts/run-tests.sh t6113-rev-list-bitmap-filters.sh`.

## Changes (summary)

- **`rev-list --objects`**: Emit trees/blobs only when not reachable from any parent commit’s trees (matches Git list-objects for ranges like `unpacked^..`).
- **Filters**: `sparse:oid=…` parsing + path matching; `includes_blob_under_tree` for `tree:N`; `TreeWalkState` for per-walk tree revisit (`tree:N`); `HEAD:path` resolves to blob roots with correct names.
- **`--use-bitmap-index`**: Interleaved commit/object printing when bitmap formatting applies; OID-only object lines when Git does; optional commit reorder when no non-commit objects (empty `tree:0` case).
- **`--unpacked`**: Skip packed commits; combine with bitmap OID-only formatting; filter packed objects from output.
- **`rev_parse`**: Expose `resolve_treeish_path` / `split_treeish_spec` for `rev:path` resolution.
- **`ignore`**: Sparse pattern helpers for `sparse:oid` filters.
