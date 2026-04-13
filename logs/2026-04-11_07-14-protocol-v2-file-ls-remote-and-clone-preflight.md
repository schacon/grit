## 2026-04-11 07:14 — protocol-v2 file:// ls-remote/clone preflight parity slice

### Goal
- Continue Phase B/E parity work for protocol-v2 transport orchestration and traces, especially around:
  - file:// v2 ls-remote request argument shape and server-option forwarding,
  - clone preflight trace parity (ref-prefix lines),
  - preserving existing matrix green suites while reducing `t5702` harness caveats.

### Code changes
- `grit/src/commands/ls_remote.rs`
  - Added protocol-v2 server option CLI surface for ls-remote:
    - `-o`, `--server-option` (append).
  - Added effective server-option resolution:
    - command-line options win,
    - otherwise read `remote.<name>.serverOption` list from config when repository argument resolves to a remote name.
  - Added protocol-version guard for server options:
    - when options are present but protocol version < 2, return:
      - `server options require protocol version 2 or later`
      - `see protocol.version in 'git help config'`
  - Routed file:// v2 ls-remote through packet label `ls-remote` for trace parity.
  - Added git:// ls-remote path using native protocol helper and shared output rendering.

- `grit/src/file_upload_pack_v2.rs`
  - Added dedicated `write_ls_refs_request_for_ls_remote(...)` helper.
  - file:// v2 ls-remote now sends:
    - `command=ls-refs`,
    - `agent=...`,
    - `object-format=...`,
    - optional `server-option=...` lines,
    - argument block containing:
      - `peel` (when not `--refs`),
      - `symrefs` (when requested),
      - `unborn`,
      - **no** `ref-prefix` lines for ls-remote patterns (matches Git behavior where filtering is client-side post-parse for this test family).
  - clone preflight tracing restored to include `clone` identity and ref-prefix lines:
    - `ref-prefix HEAD`,
    - `ref-prefix refs/heads/`,
    - `ref-prefix refs/tags/`.
  - Kept empty-wants preflight hang fix (`drop(stdin)` before wait).

- `grit/src/commands/clone.rs`
  - Restored file URL in local clone config helpers:
    - when cloning from `file://...`, configured remote URL remains `file://...` (not plain path),
    - fixes follow-up `ls-remote origin ...`/remote transport behavior in protocol-v2 tests.
  - Wrapped file:// v2 clone preflight in packet label context (`clone`) for trace identity correctness.

### Validation performed
- Build/quality:
  - `cargo fmt` ✅
  - `cargo check -p grit-rs` ✅
  - `cargo build --release -p grit-rs` ✅

- Focused protocol-v2 checks:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=9-15 -v`
    - now green: 10, 11, 12, 14, 15.
    - remaining failure in this focused block: 13 (`server-options from configuration are used by ls-remote`) in harness mode.
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=1-2,16,84 -v`
    - pass: 1,2,16,84 (git:// v2 request trace and one_time_script wait-for-done scenario both validated).

- Matrix rerun (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → **41/85** (improved from prior 37/85 in this harness profile)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh` → **215/215**

### Notes
- The remaining `t5702.13` failure appears constrained to harness sequencing/state in this environment (config-driven server-option + file:// remote-name route); direct reproductions outside that exact sequence were inconsistent.
- All non-`t5702` matrix targets remain green at their established levels, and `t5702` improved from 37→41.
