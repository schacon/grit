# t5321-pack-large-objects — test-tool `sha1 -b`

## Issue

`t5321-pack-large-objects` failed at `index-pack --stdin` with `pack trailing checksum mismatch`.

## Root cause

`lib-pack.sh` appends the pack trailer via `test-tool $(test_oid algo) -b < pack`, which must emit **raw** hash bytes (Git `test-hash` / `cmd_hash_impl`). The grit `tests/test-tool` stub treated `sha1` as `sha1sum | cut`, ignoring `-b`, so the trailer was 40 ASCII hex bytes instead of 20 binary bytes.

## Fix

Implement `test-tool sha1 -b` and `test-tool sha256 -b` using Python’s `hashlib` to stream stdin and write the raw digest (matches upstream behavior for pack trailers).

## Verification

- `bash t5321-pack-large-objects.sh` in `tests/`: 2/2 pass
- `./scripts/run-tests.sh --timeout 120 t5321-pack-large-objects.sh`: 2/2 pass
