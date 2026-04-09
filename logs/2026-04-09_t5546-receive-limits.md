# t5546-receive-limits

## Problem

Harness `t5546-receive-limits.sh` failed because:

1. Local path `git push` copied loose objects and never ran receive-side `unpack-objects` / `index-pack`, so `receive.maxInputSize` and unpack-limit routing were ignored.
2. `receive-pack` unpacked in-process without subprocess flags; `index-pack` had no `--max-input-size`.

## Fix

- Added `grit-lib` `UnpackOptions::max_input_bytes` and enforcement in `StreamingPackReader` (Git-style byte count).
- `grit unpack-objects --max-input-size`, `grit index-pack --max-input-size`.
- New `receive_ingest.rs`: shared ingest choosing `unpack-objects` vs `index-pack` from `receive.unpacklimit` / `transfer.unpacklimit` (receive wins) and `receive.maxInputSize`; used by `receive-pack` and local `push`.
- `push`: build thin pack via new `pack_objects::build_thin_push_pack`, ingest through `receive_ingest`, track new `objects/` files for hook rollback.

## Validation

- `./scripts/run-tests.sh t5546-receive-limits.sh`: 17/17
- `cargo test -p grit-lib --lib`: pass
- `cargo clippy -p grit-rs -p grit-lib --fix --allow-dirty`: clean
