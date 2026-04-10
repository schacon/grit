# t5315-pack-objects-compression

## Problem

Harness showed 5/9: `pack-objects` always used `flate2::Compression::default()` for packed object zlib streams, ignoring `core.compression` and `pack.compression`.

## Fix

- Added `ConfigSet::pack_objects_zlib_level()` in `grit-lib/src/config.rs`: walks merged config entries in order, applies `core.compression` until `pack.compression` appears (Git `pack_compression_seen` semantics). `-1` maps to level 6. Validates with `parse_git_config_int_strict`.
- `grit pack-objects` loads repo config once per run and passes `Compression::new(level)` into `build_pack` for full objects and REF_DELTA payloads.

## Verification

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5315-pack-objects-compression.sh` → 9/9
