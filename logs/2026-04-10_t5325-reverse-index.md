# t5325-reverse-index

## Goal

Make harness file `tests/t5325-reverse-index.sh` fully pass (on-disk pack reverse index / RIDX).

## Changes

- Added `grit-lib/src/pack_rev.rs`: build RIDX `.rev` from index-order offsets, parse/validate for `index-pack --verify`, `pack_rev_fsck_messages` for `fsck` (header-before-checksum ordering to match load path + multiple errors like Git), `try_rev_positions_in_pack_order` for `cat-file`.
- `ConfigSet`: `pack_write_reverse_index_default` / `pack_read_reverse_index_default` (honor `GIT_TEST_NO_WRITE_REV_INDEX`, camelCase + lowercase keys).
- `index-pack`: `--rev-index` / `--no-rev-index`, write/remove `.rev` from config + flags, `--rev-index --verify` validates existing `.rev`.
- `pack-objects`: write `.rev` when `pack.writeReverseIndex` true (default).
- `cat-file`: packed `%(objectsize:disk)` uses `.rev` when allowed; `GIT_TEST_REV_INDEX_DIE_IN_MEMORY` / `GIT_TEST_REV_INDEX_DIE_ON_DISK`.
- `fsck`: when `pack.readReverseIndex` true, validate each `.rev` next to `.idx`.

## Validation

- `./scripts/run-tests.sh --timeout 120 t5325-reverse-index.sh` → 16/16
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy --fix --allow-dirty`
