# 2026-04-11 12:20 — protocol-v2 HTTP malformed pkt-line route parity

## Scope

- Continue protocol-v2 HTTP fetch/clone parity work from the active fetch plan.
- Target high-signal failing cases where server intentionally returns malformed pkt-line payloads.

## Changes made

- Updated `grit/src/bin/test_httpd.rs`:
  - Added explicit route handling for:
    - `/smart/incomplete_length/*/git-upload-pack`
    - `/smart/incomplete_body/*/git-upload-pack`
  - For POST requests on those routes, return an `application/x-git-upload-pack-result` body with intentionally malformed pkt-line bytes:
    - incomplete length header (`00`)
    - incomplete body (`0079` + `45`)
  - This mirrors upstream test-httpd one-off malformed response behavior used by `t5702.63/.64`.

## Validation

- Build gates:
  - `cargo check -p grit-rs` ✅
  - `cargo build --release -p grit-rs` ✅
  - `cargo build -p grit-rs --bin test-httpd` ✅
- Focused protocol-v2 check:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=61-64 -v` ✅
    - `63` now fails with expected `bytes of length header were received`
    - `64` now fails with expected `bytes of body are still expected`
- Regression checks:
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` ✅ `16/16`
  - `./scripts/run-tests.sh t5510-fetch.sh` ✅ `215/215`

