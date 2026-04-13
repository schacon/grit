## 2026-04-11 01:06 UTC — fetch --unshallow local boundary sync attempt

### Scope
- Continue Phase C shallow parity focus for remaining `t5537` tail, specifically `6` (`fetch --unshallow from shallow clone`).

### Code change
- Updated `grit/src/commands/fetch.rs` unshallow branch:
  - For local/ext remotes:
    - copy reachable objects from remote tips into local ODB,
    - call `sync_shallow_boundaries_for_unshallow(...)` to align local `.git/shallow` with remote reachable boundaries.
  - For non-local remotes:
    - preserve existing fallback behavior (remove local shallow file).

### Validation
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo build --release -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- Full matrix rerun in plan order:
  - `t5702-protocol-v2`: 0/0
  - `t5551-http-fetch-smart`: no-match warning in current harness selection
  - `t5555-http-smart-common`: 10/10
  - `t5700-protocol-v1`: 24/24
  - `t5537-fetch-shallow`: 11/16
  - `t5558-clone-bundle-uri`: 27/37
  - `t5562-http-backend-content-length`: 10/16
  - `t5510-fetch`: 215/215

### Result
- No net matrix count change this iteration (`t5537` remains 11/16).
- Local unshallow path code is cleaner and explicitly routes through one sync helper for local/ext transports, but further work is still needed for the remaining `t5537` tail (`6,8,14,15,16`).
