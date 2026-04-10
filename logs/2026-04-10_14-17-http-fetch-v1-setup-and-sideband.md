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
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`
    - Result:
      - 19 passed
      - 20 passed
      - 22 still failing (post-fetch validation mismatch remains)

## Remaining issue noted
- `t5700` case 22 (`fetch with http:// using protocol v1`) remains unresolved.
- Current logs indicate fetch update lines are printed, but expected post-fetch ref/log validation does not yet align with harness expectations.
