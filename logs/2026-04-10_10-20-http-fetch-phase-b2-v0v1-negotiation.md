# HTTP fetch phase B.2 — v0/v1 stateless negotiation improvements

## Scope

- Extended the HTTP v0/v1 stateless fetch path to include local `have` negotiation derived from
  the existing skipping negotiator, instead of sending only `want` + `done`.
- Aligned packet tracing in v0/v1 HTTP mode so protocol-v1 test expectations can observe
  negotiated-version lines in `GIT_TRACE_PACKET`.

## Changes made

1. `grit/src/http_smart.rs`
   - `fetch_pack_v0_v1_stateless_http(...)` now:
     - opens the local repository and initializes `SkippingNegotiator`
     - seeds tips from local heads/tags/HEAD plus known common advertised objects
     - appends `have <oid>` lines before `done` in v0/v1 stateless POST body
   - added v0/v1 negotiated-version tracing:
     - emit `packet: git< version 1` when HTTP path runs with protocol v1
     - trace version line while parsing v0/v1 advertisements

2. `grit/src/trace_packet.rs`
   - `trace_packet_git` now honors `GIT_TRACE_PACKET=1` by writing to stderr (same behavior model
     as `trace_packet_line`), so packet traces are visible in harness logs.

## Validation run

- `cargo check -p grit-rs` ✅
- `cargo build --release -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `GIT_TRACE_CURL=1 GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22` ❌
  - file remains non-green due pre-existing setup/data dependencies in this environment
  - validated from `tests/trash.t5700-protocol-v1/log`:
    - `Git-Protocol: version=1` request headers present
    - `packet:          git< version 1` lines present in fetch trace output

## Notes

- This increment improves parity for Phase B.2 negotiation mechanics and trace observability.
- Full `t5700` pass still requires additional unrelated protocol/fixture parity work outside this
  scoped increment.
