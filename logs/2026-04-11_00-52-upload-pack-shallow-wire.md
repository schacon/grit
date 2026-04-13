# 2026-04-11 00:52 — local upload-pack shallow wire options

## Goal
Improve shallow parity in local/file transport by sending shallow/deepen directives to `upload-pack` instead of relying only on post-fetch local shallow-file editing.

## Code changes

- `grit/src/fetch_transport.rs`
  - Added `UploadPackShallowOptions` carrying:
    - `depth`
    - `deepen`
    - `shallow_since`
    - `shallow_exclude`
    - `unshallow`
  - Threaded these options through:
    - `fetch_via_upload_pack_skipping(...)`
    - `fetch_upload_pack_negotiate_pack_bytes_with_streams(...)`
  - Protocol-v0/v1 request builder now emits:
    - local `shallow <oid>` lines from `.git/shallow`
    - `deepen 2147483647` for `--unshallow`
    - `deepen <n>` for depth/deepen
    - `deepen-since <date>`
    - `deepen-not <ref>`
- `grit/src/commands/fetch.rs`
  - Build and pass `UploadPackShallowOptions` into local upload-pack negotiation path.
- `grit/src/commands/clone.rs`
  - Build and pass clone depth/since/exclude options into upload-pack clone paths (local + ssh clone code paths).
- `grit/src/ext_transport.rs`
  - Updated internal negotiate helper call to match new function signature.

## Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- `cargo test -p grit-lib --lib`
- Plan matrix checkpoint:
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → 0/0
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning (current runner selection)
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → 10/10
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → 24/24
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → 11/16
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → 27/37
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → 10/16
  - `./scripts/run-tests.sh t5510-fetch.sh` → 215/215

## Notes

- This change aligns request wiring with upstream protocol behavior for local transport shallow operations and keeps the broader matrix stable.
- Remaining `t5537` failures are still concentrated in `6/8/14/15/16`.
