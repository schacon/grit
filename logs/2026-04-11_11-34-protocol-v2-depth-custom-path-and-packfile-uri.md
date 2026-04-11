## 2026-04-11 11:34 — protocol-v2 depth/custom-path/packfile-uri cluster

### Scope
- Continue fetch-plan execution on remaining `t5702` parity gaps.
- Targeted cluster:
  - `47/48/49` (exact OID ls-refs avoidance + CLI ref-prefix/tag-following shape),
  - `52` (`--deepen` relative behavior),
  - `57/58/59/60` (custom path protocol env behavior + packfile-uris advertisement gate).

### Code changes

#### `grit/src/fetch_transport.rs`
- Added v2 optimization to skip `command=ls-refs` for CLI fetches whose sources are all explicit object IDs.
- Added helper `refspecs_are_explicit_oid_sources(...)`.
- Extended ref-prefix derivation for unqualified sources to emit both:
  - raw token (`ref-prefix dwim`),
  - heads namespace (`ref-prefix refs/heads/dwim`).
- Threaded `include_tag` control into v2 fetch request writing.

#### `grit/src/file_upload_pack_v2.rs`
- Extended `write_v2_fetch_request(...)`:
  - `include-tag` support,
  - `deepen-relative` support when requested.
- Updated clone preflight callsites for new signature.

#### `grit/src/commands/fetch.rs`
- Local upload-pack CLI path now uses selective follow-tag expansion (`append_follow_tags_for_wants`) instead of blindly wanting all remote tag refs.
- Added deepen-depth estimator for local transports:
  - uses current shallow depth + advance distance for `--deepen`,
  - passes `replace_existing_boundaries` mode to shallow writer for deepen.
- Refined shallow boundary writer to support two modes:
  - preserve existing boundaries (`--depth`),
  - replace ancestor boundaries for fetched tips (`--deepen`),
  fixing `t5537.5` regression while keeping `t5702.52` green.

#### `grit/src/pack_objects_upload.rs`
- For depth-limited upload-pack responses, disable `--not` exclusion stdin in `pack-objects` process setup and rely on rev-list `--not` filtering via emitted revision stdin.
- Prevents thin/no-objects responses in relative deepen scenarios.

#### `grit/src/commands/send_pack.rs`
- Custom `--receive-pack` command path now only forwards `GIT_PROTOCOL` for the default receive-pack command.
- Shell/explicit custom commands no longer receive protocol-v2 env by default (matches `t5702.57`).

#### `grit/src/commands/archive.rs`
- Remote archive now resolves remote names through `remote.<name>.url` before spawning upload-archive.
- Custom `--exec` with `git-upload-archive` now rewrites to the current grit binary upload-archive subcommand while preserving shell wrappers (`env >../env.trace; ...`), and clears `GIT_PROTOCOL`.
- This fixes `t5702.59` for custom exec path behavior.

#### `grit/src/commands/serve_v2.rs`
- Added packfile-uris advertisement gate:
  - advertise `fetch ... packfile-uris` only when `uploadpack.blobpackfileuri` is configured with a non-empty value.
  - reject `packfile-uris ...` request lines unless advertised.
- Added handling for valueless config entries by parsing keys without `=` as empty string in config parser.

### Validation

#### Quality gates
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` ✅ (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅

#### Focused protocol-v2 checks
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=44-49 -v` ✅
  - fixed: `47`, `48`, `49`.
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=50-52 -v` ✅
  - fixed: `52`.
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=55-60 -v` ✅
  - fixed: `57`, `58`, `59`, `60`.
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-52 -v` ✅

#### Matrix checkpoint (ordered)
- `./scripts/run-tests.sh t5702-protocol-v2.sh` → **70/85** (improved from 62/85 baseline at turn start)
- `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → **31/37**
- `./scripts/run-tests.sh t5555-http-smart-common.sh` → **10/10**
- `./scripts/run-tests.sh t5700-protocol-v1.sh` → **24/24**
- `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **16/16**
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **37/37**
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **16/16**
- `./scripts/run-tests.sh t5510-fetch.sh` → **215/215**

### Remaining t5702 failures after this increment
- `63, 64, 67, 68, 69, 72, 73, 74, 76, 77, 78, 79, 80, 81, 82`
