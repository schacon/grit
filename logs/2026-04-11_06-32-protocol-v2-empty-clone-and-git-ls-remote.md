## 2026-04-11 — protocol-v2 transport slice (empty clone preflight + git:// ls-remote)

### Goal
- Continue fetch-plan execution by removing newly observed protocol-v2 transport regressions:
  - `file://` empty-repo clone hanging in v2 preflight (`t5702.16` path),
  - missing native `git://` `ls-remote` handling for protocol-v2 trace tests (`t5702.2` path),
  - trace identity mismatch side-effects in bundle negotiation (`t5558` clone trace expectations).

### Changes implemented

#### 1) Prevent empty `file://` clone preflight deadlock
- **File:** `grit/src/file_upload_pack_v2.rs`
- In `clone_preflight_file_v2_if_needed(...)`:
  - when `ls-refs` yields no wants, explicitly `drop(stdin)` before `child.wait()`.
  - this lets server-side `upload-pack` exit instead of waiting for another v2 command.
- Effect: empty repository clone no longer blocks indefinitely.

#### 2) Make v2 packet tracing use active command identity
- **File:** `grit/src/file_upload_pack_v2.rs`
- Replaced direct `trace_packet::trace_packet_git` usage with wrapper that calls:
  - `wire_trace::trace_packet_line_ident(trace_packet::negotiation_packet_label(), ...)`
- Effect:
  - traces now follow active label context (`clone`, `fetch`, `ls-remote`) instead of always `git`,
  - restored expected `clone>` lines in bundle negotiation tests and `ls-remote>` lines in protocol tests.

#### 3) Add native `git://` `ls-remote` transport path
- **Files:**
  - `grit/src/fetch_transport.rs`
  - `grit/src/commands/ls_remote.rs`
- Added `fetch_transport::ls_remote_via_git_protocol(url)`:
  - opens git:// socket,
  - sends upload-pack service request with protocol version header,
  - parses advertisement, and for v2 performs `ls-refs` request.
- Wired `commands/ls_remote::run(...)`:
  - detect `git://` URLs,
  - route through `with_packet_trace_identity("ls-remote", ...)`,
  - print filtered refs via common advertised-ref printer.

#### 4) Keep clone preflight trace identity stable
- **File:** `grit/src/commands/clone.rs`
- Wrapped `file://` v2 preflight call in:
  - `fetch_transport::with_packet_trace_identity("clone", || ...)`
- Effect: preserves `clone>` trace labels used by `t5558` negotiation assertions.

### Validation evidence

#### Build/quality gates
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` ✅  
  (reverted unrelated `grit-lib/src/repo.rs` change)
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅

#### Focused protocol-v2 checks
- `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-2,16,84 -v` ✅
  - `t5702.2`: git:// ls-remote v2 packet trace now includes expected request and response version lines.
  - `t5702.16`: empty repo clone via file:// v2 completes (`done.`), no hang.
  - `t5702.84`: one_time_script wait-for-done failure remains correct.

#### Matrix suites
- `./scripts/run-tests.sh t5702-protocol-v2.sh` → **37/85**
- `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → **31/37**
- `./scripts/run-tests.sh t5555-http-smart-common.sh` → **10/10**
- `./scripts/run-tests.sh t5700-protocol-v1.sh` → **24/24**
- `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **16/16**
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **37/37**
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **16/16**
- `./scripts/run-tests.sh t5510-fetch.sh` → **215/215**

### Notes
- `t5702` now executes and reports granular outcomes instead of the prior 0/0 timeout profile.
- Remaining `t5702` failures are broader protocol-v2 parity gaps outside this targeted transport slice.
