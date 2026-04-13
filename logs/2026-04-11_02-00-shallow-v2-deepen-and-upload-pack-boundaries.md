## 2026-04-11 02:00 UTC — shallow v2 deepen wire + upload-pack shallow boundary handling

### Scope
- Continue Phase C shallow parity work with focus on `t5537-fetch-shallow.sh` tail.
- Targeted gap: depth/unshallow behavior over local upload-pack protocol-v2 path and shallow client boundary handling.

### Code changes

#### `grit/src/file_upload_pack_v2.rs`
- Extended `write_v2_fetch_request(...)` to emit shallow/deepen directives:
  - `shallow <oid>` for local boundary OIDs,
  - `deepen <n>`,
  - `deepen-since <date>`,
  - `deepen-not <ref>`,
  - `deepen 2147483647` for `--unshallow`.
- Updated call sites (including clone preflight path) for new parameters.

#### `grit/src/fetch_transport.rs`
- In v2 upload-pack negotiation path, threaded local shallow options into `write_v2_fetch_request(...)`:
  - local shallow OIDs from `.git/shallow`,
  - effective depth/deepen/since/exclude/unshallow options.

#### `grit/src/pack_objects_upload.rs`
- Added reusable helper `compute_depth_exclude_commits(repo, wants, depth)` to derive parent OIDs just beyond depth boundary.
- Kept `write_pack_objects_revs_stdin(...)` semantics but generalized to exclusion commit list naming.

#### `grit/src/commands/upload_pack.rs`
- Parse client `shallow <oid>` and `deepen <n>` request lines.
- During `have` closure construction, stop ancestry traversal at client shallow boundaries.
- Build exclusion commits for `pack-objects --revs` as:
  - `have` commits when client is not shallow,
  - plus depth boundary exclusions from `compute_depth_exclude_commits(...)`.
- Tightened thin-pack gating to avoid unsafe exclusion assumptions when client is shallow.

#### `grit/src/commands/serve_v2.rs`
- Accept `deepen <n>` in v2 fetch command parsing.
- Apply depth boundary exclusion commits to pack generation in v2 server path.

#### `grit/src/commands/fetch.rs`
- Local/ext `--unshallow` object copy now uses source-shallow-aware traversal helper:
  - copy commit/tree/tag graph from fetched tips,
  - do not traverse parent commits beyond source shallow boundary commits.
- `sync_shallow_boundaries_for_unshallow(...)` local boundary initialization remains reset-to-empty for re-synchronization with remote reachable boundaries.

### Validation
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` ✅
  - reverted unrelated `grit-lib/src/repo.rs` clippy edit
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅

### Focused runtime checks
- `GIT_TRACE_PACKET=1 GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-2 -v`
  - verified v2 fetch request includes `deepen 2`.
- `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-8 -v`
  - tests 1..7 pass; test 8 still fails.

### Matrix checkpoint (plan order)
- `./scripts/run-tests.sh t5702-protocol-v2.sh` → **0/0**
- `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning in current harness selection
- `./scripts/run-tests.sh t5555-http-smart-common.sh` → **10/10**
- `./scripts/run-tests.sh t5700-protocol-v1.sh` → **24/24**
- `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **12/16** (improved from 11/16)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **27/37**
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **10/16**
- `./scripts/run-tests.sh t5510-fetch.sh` → **215/215**

### Remaining `t5537` tail
- Still failing: **8, 14, 15, 16**
