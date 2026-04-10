# HTTP fetch phase B.1 — v0/v1 discovery fallback

## Scope

- Implemented protocol discovery fallback in `grit/src/http_smart.rs` so HTTP fetch can parse
  non-v2 smart advertisements and attempt a stateless v0/v1 fetch path.
- Kept v2 flow intact as the fast path.

## Changes made

1. Added advertisement parsing + protocol discrimination:
   - `parse_v0_v1_advertisement(...)`
   - `HttpDiscovery` enum (`V2` vs `V0V1`)
   - `discover_http_protocol(...)`

2. Added v0/v1 fetch request capability shaping:
   - `build_fetch_caps_v0(...)` (filters to capabilities actually advertised).

3. Added v0/v1 stateless fetch RPC attempt:
   - `fetch_pack_v0_v1_stateless_http(...)`
   - Sends `want` lines + `done`, reads initial ACK/NAK, demuxes sideband if enabled, and unpacks
     pack bytes via existing transport unpack path.

4. Integrated discovery in `http_ls_refs` and `http_fetch_pack`:
   - `http_ls_refs`: returns advertised refs directly for v0/v1.
   - `http_fetch_pack`: falls back to v0/v1 stateless RPC when discovery is non-v2.

## Validation run

- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `./scripts/run-tests.sh t5700-protocol-v1.sh` ❌ (still 9/24)
  - HTTP-focused subset (`--run=20,21,22,23,24`) still failing in current environment.
  - Current failure mode appears influenced by broader harness/environment instability in direct
    targeted runs (e.g., missing expected setup artifacts under partial `--run` sequences), not
    yet conclusive proof the new fallback path is correct end-to-end.

## Notes

- This increment lands protocol fallback plumbing and parser paths required by plan Phase B.1.
- Additional Phase B.2/B.3 work is still needed to match Git's full v0/v1 fetch-pack behavior.
