## Summary

Implemented protocol-v2 server-option parity across `fetch` and `clone` local/file upload-pack paths, and fixed regressions introduced by narrowed v2 `ls-refs` prefixes.

## Code changes

### `grit/src/commands/fetch.rs`

- Added fetch CLI support for protocol-v2 server options:
  - `-o, --server-option <opt>` (appendable).
- Added `effective_fetch_server_options(...)`:
  - command-line server options override config values,
  - reads `remote.<name>.serverOption` with empty-value reset semantics,
  - rejects missing-value config entries with:
    - `error: missing value for 'remote.<name>.serveroption'`,
  - enforces protocol gate:
    - `server options require protocol version 2 or later` +
      `see protocol.version in 'git help config'`.
- Threaded server options through local upload-pack fetch negotiation.
- For `fetch --all`, ensured each remote receives independent protocol-v2 option forwarding by:
  - passing `all_refspecs` and `server_options` into transport,
  - using all-refspec-based v2 ref-prefix derivation in ls-refs prefetch.

### `grit/src/commands/clone.rs`

- Added clone server-option config handling with remote-aware key lookup:
  - `effective_clone_server_options(args, remote_name)`.
- Preserved precedence:
  - CLI `--server-option` overrides config,
  - `-c remote.<name>.serverOption=` clears previous values.
- Added validation for invalid config entry form:
  - `-c remote.origin.serverOption` (no value) now fails with:
    - `error: missing value for 'remote.origin.serveroption'`.
- Threaded resolved server options into:
  - file:// v2 preflight fetch path,
  - local/ssh upload-pack clone fetch path.

### `grit/src/fetch_transport.rs`

- Extended `v2_ls_refs_for_fetch(...)` to derive `ref-prefix` entries from refspecs, including:
  - `refs/heads/*`, `refs/tags/*`, direct refs, and pseudo/short refs.
- Added special handling in `v2_ref_prefixes_from_refspecs(...)`:
  - `tag <name>` now maps to `refs/tags/<name>` prefix,
  - this restored shallow-tag update behavior in `t5537`.
- Extended `fetch_via_upload_pack_skipping(...)` signature to accept:
  - `refspecs` and `server_options`.
- Updated v2 command generation:
  - server options are now written only on `command=fetch` requests, not `ls-refs`,
    matching `t5702.26` expectations (exactly one `server-option` trace per remote fetch).

### `grit/src/file_upload_pack_v2.rs`

- Extended `write_v2_fetch_request(...)` to accept `server_options` and emit
  `server-option=<opt>` lines in the fetch command block.
- Extended `clone_preflight_file_v2_if_needed(...)` signature to carry server options
  into preflight fetch command generation.

## Focused validation

- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅

- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=9-33 -v`
  - fixed and passing:
    - `24`, `25`, `26`, `27`, `28`, `29`, `30`, `31`, `32`, `33`
  - remaining failures in this focused span:
    - `17`, `19` (pre-existing unborn-HEAD propagation behavior; unrelated to this server-option slice).

- `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`
  - `16/16` pass after restoring `tag <name>` ref-prefix derivation.

## Matrix checkpoint

- `./scripts/run-tests.sh t5702-protocol-v2.sh` → `52/85`
- `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → `31/37`
- `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
- `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
- `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `16/16`
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `37/37`
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → `16/16`
- `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`

