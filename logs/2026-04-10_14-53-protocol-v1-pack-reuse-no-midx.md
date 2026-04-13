## Phase slice
- Protocol-v1 fetch/clone stabilization (local/file transport) after v1 HTTP framing fixes.

## Root cause
- `t5700-protocol-v1.sh` file-transport clone/fetch paths were failing with:
  - `error: corrupt object: no multi-pack-index found`
  - `error: pack-objects failed with exit code 1`
- The failure came from `pack-objects` multi-pack reuse startup:
  - `compute_midx_reused_entries()` called `read_midx_pack_idx_names()` and treated
    missing MIDX (`no multi-pack-index found`) as a hard error.
  - On repositories without a MIDX (common in test setup), this should degrade to
    "no reuse", not abort pack generation.

## Code changes
- File: `grit/src/commands/pack_objects.rs`
- In `compute_midx_reused_entries()`:
  - Treat `LibError::CorruptObject("no multi-pack-index found")` as `Ok(None)`.
  - Keep other MIDX read errors as hard failures.

This preserves reuse when MIDX exists, while matching Git behavior on repositories
that only have regular pack indexes and no multi-pack-index.

## Validation
- Build/check:
  - `cargo check -p grit-rs` ✅
  - `cargo build --release -p grit-rs` ✅
- Focused suite:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=6,7,11`
    - all selected tests passed.
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=6,7,8,9,10,13,14,15,16,17,19,20,22`
    - file:// and HTTP selected cases passed
    - remaining failures are SSH subset (`14-17`), outside this specific MIDX/no-MIDX fix.
