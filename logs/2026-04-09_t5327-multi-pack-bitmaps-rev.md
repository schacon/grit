# t5327-multi-pack-bitmaps-rev

## Summary

Made `tests/t5327-multi-pack-bitmaps-rev.sh` pass (314/314).

## Key fixes

- **fast-import**: Update symbolic `HEAD` → branch ref instead of detaching (`test_commit_bulk` + `branch -M`).
- **rev-list**: Bitmap test trace2 + OID-only object lines for bitmap traversal.
- **midx**: External `.rev` sidecar, `--no-bitmap`, `--stdin-packs`, `--preferred-pack`; `test-tool read-midx --show-objects`.
- **Clone/fetch**: Absolute local `remote.origin.url`; `--no-local` bare writes refs + HEAD; skip “checked out branch” guard on bare repos; fetch negotiation skips non-commit tips (tagged blob); `GIT_DIR` for pack-objects child.
- **pack-objects**: Ref reachability for `--all` counts; progress `Enumerating objects: N, done.` + `pack-reused`; tree-referenced external blob bases (capped); short→long delta pairing; embed missing REF_DELTA bases; Case A prefix search same-pack only.
- **upload-pack / fetch transport**: Filter negotiation, `store_received_pack` via index-pack, etc.

## Validation

- `./scripts/run-tests.sh t5327-multi-pack-bitmaps-rev.sh` → 314/314
- `cargo test -p grit-lib --lib`
