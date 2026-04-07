# t11500-add-chmod-intent

## Issue

Harness showed 29/31: tests expected `ls-files -s` to show `0000000000000000000000000000000000000000` for intent-to-add placeholders; Grit used the empty blob OID (`e69de29…`).

## Fix

- `grit add -N` / `apply --intent-to-add`: set index OID to null via `ObjectId::zero()`.
- `reset -N` path reset: same for synthetic intent-to-add entries (no longer write empty blob to ODB for that case).
- Added `ObjectId::zero()` on `grit-lib` for a single canonical null OID.

## Verification

- `./scripts/run-tests.sh t11500-add-chmod-intent.sh` → 31/31
- `cargo test -p grit-lib --lib`
