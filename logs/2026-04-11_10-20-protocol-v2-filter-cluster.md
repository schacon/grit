## 2026-04-11 10:20 — protocol-v2 filter cluster (t5702.37/39/40/42)

### Scope
- Continue Phase C.3 / E.4 parity work for `t5702-protocol-v2.sh` filter/lazy-fetch failures.
- Fixes targeted:
  - `37` dynamically fetch missing object should use protocol v2 and succeed
  - `39` partial fetch with `--filter=blob:none`
  - `40` filter capability advertisement must honor `uploadpack.allowfilter`
  - `42` handcrafted `filter` request must fail when filter is not advertised

### Changes made
- `grit/src/commands/serve_v2.rs`
  - `ServerCaps` now loads `uploadpack.allowfilter`.
  - `fetch=` capability line advertises `filter` only when allowed.
  - `cmd_fetch` now rejects `filter ...` lines when filter was not advertised.
  - Parsed `filter` argument is forwarded to pack generation path.

- `grit/src/pack_objects_upload.rs`
  - `spawn_pack_objects_upload(...)` now accepts optional `filter_spec`.
  - Passes `--filter <spec>` through to `grit pack-objects` / hook invocation.

- `grit/src/commands/upload_pack.rs`
  - v0/v1 upload-pack path now parses incoming `filter ...` line and forwards it into
    `spawn_pack_objects_upload(..., filter_spec)` so filtered packs are actually generated.

- `grit/src/fetch_transport.rs`
  - `fetch_upload_pack_explicit_wants(...)` now accepts optional `filter_spec`.
  - Explicit-wants lazy fetch now negotiates protocol v2 correctly when configured:
    reads v2 capability block and sends `command=fetch` request for wants.
  - `fetch_via_upload_pack_skipping(...)` and lower negotiation helper now thread optional
    `filter_spec`, and v2 request writer now emits `filter <spec>` in fetch requests.
  - Updated ext/git callsites for changed helper signatures.

- `grit/src/file_upload_pack_v2.rs`
  - `write_v2_fetch_request(...)` now accepts optional `filter_spec` and writes `filter ...`.

- `grit/src/commands/fetch.rs`
  - Thread non-empty CLI `--filter` into upload-pack skipping path (`pack_filter_spec`).

- `grit/src/commands/clone.rs`
  - Thread non-empty clone `--filter` into upload-pack clone paths.

- `grit/src/commands/promisor_hydrate.rs`
  - Reads `remote.<name>.partialclonefilter` and passes it to explicit-wants fetch.
  - Added fallback retry without filter when filtered explicit-wants fetch returns an empty pack
    and object is still missing, to preserve lazy-hydration behavior while retaining v2 tracing.

- `grit/src/commands/rev_list.rs`
  - `--quiet --objects` now still emits object/missing-object lines while suppressing commit
    lines. This matches `t5702.39` expectations (`rev-list --quiet --objects --missing=print`).

### Validation run evidence
- Build/quality:
  - `cargo fmt`
  - `cargo check -p grit-rs`
  - `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`
  - `cargo test -p grit-lib --lib`
  - `cargo build --release -p grit-rs`

- Focused:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=35-42 -v`
    - all pass in focused run (`37/39/40/42` now green).
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=35,39 -v`
    - pass; shows `?8045...` missing blob line for partial fetch case.

- Matrix checkpoint:
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → `62/85`
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → `31/37`
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `16/16`
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `37/37`
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → `16/16`
  - `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`
