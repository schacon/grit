# t5308 pack trailer / pack_obj

## Problem

`t5308-pack-detect-duplicates` failed because hand-built packs had an invalid trailer: `pack_trailer` runs `test-tool sha1 -b` (or sha256), but the tests `test-tool` stub only handled `sha1`/`sha256` as stdin→hex digest, so it fell through to `git test-tool` and wrote garbage instead of 20 bytes of SHA-1.

Secondary: after a test calls `test_oid_cache`, `pack_obj` no longer matched `$(test_oid packlib_7_76)` for the lo blob; added explicit sha1 hex in `lib-pack.sh` case arm.

## Fix

- `tests/test-tool`: implement `sha1 -b` / `sha256 -b` via Python (binary hash of stdin).
- `tests/lib-pack.sh`: match `e68fe8129b546b101aee9510c5328e7f21ca1d18` alongside `packlib_7_76`.

Harness: `./scripts/run-tests.sh t5308-pack-detect-duplicates.sh` → 6/6.
