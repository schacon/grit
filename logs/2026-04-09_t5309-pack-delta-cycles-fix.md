# t5309-pack-delta-cycles fix

## Root cause

- `lib-pack.sh` `pack_trailer` runs `test-tool $(test_oid algo) -b` expecting **raw 20-byte** SHA-1 like Git’s `test-hash`; our stub printed **hex**, corrupting pack trailers → `index-pack` reported checksum mismatch on tests 1–2 and downstream cases.
- Test 7 uses `test-tool -C server pack-deltas …`; the shell `tests/test-tool` script did not forward `pack-deltas` to grit, so thin.pack stayed empty and `clone`/`index-pack` failed.

## Changes

- `tests/test-tool`: implement `sha1`/`sha256` with optional `-b` (binary digest via Python hashlib); delegate `pack-deltas` to `$GUST_BIN` with `-C` when set.
- `grit`: add `test_tool_pack_deltas` (matches `git/t/helper/test-pack-deltas.c`: REF_DELTA lines, zlib level 1, pack v2 header + SHA-1 trailer); parse `--num-objects=2` as well as separate `-n` / `--num-objects` forms; wire in `test-tool` dispatch.

## Verification

`./scripts/run-tests.sh t5309-pack-delta-cycles.sh` → 7/7 pass.
