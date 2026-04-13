# HTTP fetch Phase A.1 — Git-Protocol header control

## Scope

Implemented the Phase A.1 slice from the fetch plan: make `Git-Protocol` header behavior explicit/configurable and prepare per-request control for HTTP fetch/clone.

## Code changes

### `grit/src/http_client.rs`

- Added protocol-header state on `HttpClientContext`:
  - `git_protocol_header: Option<String>`
- Added protocol-version resolution:
  - read `protocol.version` from config when available
  - fallback to `protocol_wire::effective_client_protocol_version()`
  - map `0 -> no header`, `1 -> Git-Protocol: version=1`, otherwise `version=2`
- Added per-request API:
  - `get_with_git_protocol(..., Option<&str>)`
  - `post_with_git_protocol(..., Option<&str>)`
  - existing `get`/`post` now use context default header value
- Updated all HTTP execution paths (ureq / HTTP proxy / SOCKS) to accept optional `Git-Protocol` header, instead of hard-coding `version=2`.
- Trace output now includes `Git-Protocol: ...` when present.

### `grit/src/commands/clone.rs`

- Updated HTTP clone config loading for pre-repo context:
  - switched from manual global-only merge to `ConfigSet::load(None, true)` so command/env overrides (including `GIT_CONFIG_PARAMETERS` from `git -c ...`) are visible to the HTTP client configuration.
  - still applies clone-local `args.config` overrides after base load.

## Validation

- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅ (166 tests)
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh`:
  - overall: 9/24 (many pre-existing failures)
  - relevant check for this slice: HTTP v1 clone test remained green and validates `Git-Protocol: version=1` in trace output.

## Notes

- This commit intentionally focuses on header control plumbing only.
- Credential flow and broader HTTP fetch parity remain in-progress and are tracked in subsequent Phase A/B work.
