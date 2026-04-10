## Phase slice
- Fetch plan execution: protocol-v1 HTTP stabilization and harness setup parity.

## Changes implemented

### 1) Remove implicit root-commit creation from `git add`
- File: `grit/src/commands/add.rs`
- Removed auto-commit helper logic that wrote an initial commit from the index when `HEAD` was unborn.
- Rationale:
  - This behavior is non-Git-compatible for `add`.
  - It caused harness setup sequences (`test_commit`) to fail with:
    - `error: nothing to commit, working tree clean`
  - After removal, setup test 19 in `t5700-protocol-v1.sh` (`create repos to be served by http:// transport`) now passes.

### 2) Harden HTTP v0/v1 side-band pack parser
- File: `grit/src/http_smart.rs`
- Changes:
  - In `fetch_pack_v0_v1_stateless_http`, when `side-band-64k` is enabled, do **not** pre-read the first pkt-line as text.
    - This first packet may carry binary channel-1 pack bytes.
  - Reworked `read_sideband_pack_until_done`:
    - reads raw pkt lengths directly,
    - ignores flush packets before pack start,
    - retains a rolling buffer and detects `PACK` magic across packet boundaries,
    - keeps channel demux semantics (1=data, 2/3=progress/error).

## Validation executed
- `cargo fmt`
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`
- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib`
- Focused protocol suites:
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=83,84,85`
  - Result: all passed (85/85 overall in that run)
  - Note: this required building debug `test-httpd` (`cargo build -p grit-rs --bin test-httpd`) so
    `tests/lib-httpd.sh` could locate the binary at `target/debug/test-httpd`; otherwise the suite
    can fail before command behavior is exercised.
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`
    - Result:
      - 19 passed
      - 20 passed
      - 22 still failing (post-fetch validation mismatch remains)

## Remaining issue noted
- `t5700` case 22 (`fetch with http:// using protocol v1`) remains unresolved.
- Current logs indicate fetch update lines are printed, but expected post-fetch ref/log validation does not yet align with harness expectations.

## Follow-up fix in same phase slice
- Adjusted HTTP v0/v1 request framing to match `fetch-pack` behavior:
  - after writing the `want` block, now send a pkt-line **flush** (`0000`) before negotiation (`have` lines) and `done`.
  - this avoids servers treating `done` as part of the wants section in stateless HTTP v1 paths.
- Kept the retry-without-haves fallback for endpoints that ACK without pack on the first round.

## Follow-up validation
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`:
  - now passes all selected tests (`19,20,22`).
- `./scripts/run-tests.sh t5700-protocol-v1.sh`:
  - improved to `15/24` in this environment (from prior `14/24` and original `9/24` baseline).
