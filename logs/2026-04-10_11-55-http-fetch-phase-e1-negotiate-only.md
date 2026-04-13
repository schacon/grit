## 2026-04-10 11:55 — Phase E.1 (`--negotiate-only`) implementation

### Goal
- Replace `fetch --negotiate-only` stub with real negotiation-only behavior and align error semantics with `t5702` protocol tests.

### Code changes

#### `grit/src/commands/fetch.rs`
- Added real negotiate-only CLI surface:
  - `--negotiation-tip` argument (`Args.negotiation_tip: Vec<String>`).
- Added Git-compatible fatal validation path before running fetch transport:
  - `--negotiate-only` requires one or more `--negotiation-tip=*`.
  - `--negotiate-only` is incompatible with recurse-submodules when requested on CLI.
  - Protocol version must be v2 (resolved from config and env-parity path).
- Added helpers:
  - `exit_fatal(...)` for Git-shaped fatal+exit(128) behavior.
  - `parse_protocol_version(...)`.
  - `resolve_negotiation_tip_oids(...)`:
    - supports literal OIDs, revs, and glob tips (`*_1` patterns).
    - errors on missing OIDs with Git-style fatal message.
  - `negotiate_only_common_with_remote_repo(...)`:
    - computes common commits against local/file remotes via merge-base.
- Integrated negotiate-only execution in `fetch_remote(...)`:
  - HTTP path delegates to `http_smart::http_negotiate_only_common`.
  - Local/file path uses merge-base common computation.
  - Prints common OIDs to stdout and returns without pack transfer.

#### `grit/src/http_smart.rs`
- Added `http_negotiate_only_common(...)`:
  - smart HTTP advertisement discovery;
  - enforces protocol v2;
  - verifies `wait-for-done` capability in advertised v2 fetch features;
  - computes common OIDs from local negotiation tips vs advertised remote refs.

#### `grit/src/commands/pull.rs`
- Updated fetch args construction to initialize `negotiation_tip` (empty vec) for compile completeness.

### Validation

- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=53,54,55,56,83,84,85`
  - `53/54/55/56` pass (usage + file:// negotiate-only behavior)
  - `83` pass (`http:// --negotiate-only`)
  - `84` still fails in this harness environment due to `one_time_script` setup mismatch in the custom HTTP test server path (observed in verbose run), not due to missing negotiate-only implementation.
- Regression checks for ongoing fetch-plan targets:
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → `9/24`
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `13/37`
  - no regression from prior baseline in this workspace.
