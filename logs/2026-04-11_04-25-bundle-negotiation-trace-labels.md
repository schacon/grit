## Summary

Improved bundle-uri negotiation trace parity for clone/fetch paths by fixing packet-trace label propagation.

## Change implemented

- File: `grit/src/fetch_transport.rs`
- Function: `with_packet_trace_identity(...)`
- Change:
  - Wrapped execution with `trace_packet::with_packet_trace_label(identity, ...)` in addition to the existing wire-level packet identity guard.
  - This ensures helper traces emitted via `trace_fetch_tip_availability(...)` use the same identity label (`clone>` vs `fetch>`) as wire packet traces.

## Why

Several `t5558-clone-bundle-uri.sh` negotiation assertions grep for `clone> have <oid>` lines in `GIT_TRACE_PACKET` output when cloning with bundles.

Before this change, those lines were emitted as `fetch> ...` because negotiation-label state was not synchronized with `with_packet_trace_identity("clone", ...)`.

## Validation

- Build gates:
  - `cargo fmt`
  - `cargo check -p grit-rs`
  - `cargo test -p grit-lib --lib`
  - `cargo build --release -p grit-rs`
- Focused:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5558-clone-bundle-uri.sh --run=1-20 -v` ✅
- Matrix:
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` improved **27/37 → 30/37**
  - full ordered matrix rerun recorded in `test-results.md`.

