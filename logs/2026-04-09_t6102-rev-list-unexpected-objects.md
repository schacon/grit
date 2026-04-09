## 2026-04-09 — t6102-rev-list-unexpected-objects (seen non-blob)

- **Issue:** Harness showed 21/22; test 4 (`traverse unexpected non-blob entry (seen)`) expected `rev-list --objects` to fail with `is not a blob` when the malformed tree’s “blob” slot pointed at an OID already listed from another tip.
- **Cause:** `collect_tree_objects_filtered` treated non-blob children in blob modes as tolerable and skipped output when the OID was already in `emitted`, so the walk succeeded with no error.
- **Fix:** If the child OID is already in `emitted`, return `CorruptObject` with message `object <oid> is not a blob` (matches Git `list-objects.c` / t6102). Lone-tip case unchanged (OID not yet seen).
- **Validation:** `cargo test -p grit-lib --lib`; `./scripts/run-tests.sh t6102-rev-list-unexpected-objects.sh` → 22/22.
