# t5705-session-id-in-capabilities

## Summary

Implemented Git-compatible trace2 `transfer` events and wire `session-id` handling so `tests/t5705-session-id-in-capabilities.sh` passes 17/17.

## Key changes

- **`trace2_transfer`**: `negotiated-version`, `server-sid`, `client-sid`, `transfer.advertiseSID` lookup, per-process wire session id.
- **Fetch**: `effective_client_protocol_version()` drives `GIT_PROTOCOL` on upload-pack child; v2 fetch path (ls-refs + fetch + close stdin to avoid deadlock); strip `GIT_TRACE2*` from upload-pack children; strip quotes from parsed env in `parse_leading_shell_env_assignments`.
- **upload-pack / receive-pack / serve_v2**: advertise `session-id=` when config enabled; parse client session id on v0 wants and v2 fetch; emit `negotiated-version` from server `GIT_PROTOCOL`.
- **send_pack / push**: resolve `HEAD:new-branch`; spawn `grit receive-pack` when template contains `git-receive-pack`; emit trace2 from advertisement; remove early delegation to system git for `--receive-pack` on local push.

## Validation

- `./scripts/run-tests.sh t5705-session-id-in-capabilities.sh` → 17/17
- `cargo test -p grit-lib --lib` → pass
- `cargo check -p grit-rs` → pass
