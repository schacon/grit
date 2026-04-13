# HTTP fetch phase C.4 — `--refetch` parity in smart HTTP

## Scope

- Implemented HTTP fetch behavior for `--refetch` so smart-HTTP requests skip
  `have`/known-common negotiation and request a full transfer shape closer to
  fresh-clone semantics.
- Wired this through both `fetch` and `clone` call sites using
  `HttpFetchOptions`.

## Changes made

1. Extended `HttpFetchOptions` (`grit/src/http_smart.rs`):
   - Added `refetch: bool`.

2. Wired options from callers:
   - `grit/src/commands/fetch.rs` sets:
     - `refetch: args.refetch`
   - `grit/src/commands/clone.rs` sets:
     - `refetch: false`
   - Existing depth/deepen/shallow/filter options stay intact.

3. Updated v0/v1 HTTP fetch negotiation path:
   - `fetch_pack_v0_v1_stateless_http(...)` now conditionally bypasses
     `SkippingNegotiator` setup when `options.refetch` is true.
   - In refetch mode, no `have` lines are emitted.

4. Updated v2 HTTP fetch negotiation path:
   - `http_fetch_pack(...)` now conditionally bypasses negotiator tip/common
     discovery when `options.refetch` is true.
   - In refetch mode, request builder emits no `have` lines before `done`.

## Validation run

- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `./scripts/run-tests.sh t5702-protocol-v2.sh` ⚠️ `0/0` (existing harness state)
- `./scripts/run-tests.sh t5700-protocol-v1.sh` ❌ `9/24` (unchanged aggregate)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` ❌ `13/37` (unchanged aggregate)
- `./scripts/run-tests.sh t5537-fetch-shallow.sh` ❌ `0/16` (unchanged aggregate)

## Notes

- This increment focuses on request-shape parity for HTTP `--refetch`.
- Existing broad failures in protocol/bundle/shallow suites remain, but this
  lands the explicit refetch transport behavior required by Phase C.4.
