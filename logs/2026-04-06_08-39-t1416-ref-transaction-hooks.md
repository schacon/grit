## t1416-ref-transaction-hooks (2026-04-06 08:39)

### Goal
Close the remaining 1 failing case in `t1416-ref-transaction-hooks.sh` (`hook gets all queued symref updates`).

### Root cause
- The local test harness helper `test_hook` in `tests/test-lib.sh` parsed `--setup` but did not actually apply lifecycle behavior for non-setup hooks.
- As a result, hooks created in earlier tests persisted into later tests unless explicitly removed.
- In `t1416`, the `reference-transaction` hook from test 9 leaked into test 10 setup commands (`git update-ref`, `git symbolic-ref ...`), appending unexpected entries to `actual` before the queued `update-ref --stdin` transaction under test.

### Changes made
- Updated `tests/test-lib.sh`:
  - `test_hook` now registers `test_when_finished "rm -f \"$hook_file\""` when `--setup` is not provided.
  - Keeps existing behavior for setup hooks while ensuring per-test hook isolation by default.

### Validation
- `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t1416-ref-transaction-hooks.sh` (from `/workspace/tests`): **10/10**.
- `./scripts/run-tests.sh t1416-ref-transaction-hooks.sh`: **10/10**.
- Regressions:
  - `./scripts/run-tests.sh t1403-show-ref.sh`: **12/12**.
  - `./scripts/run-tests.sh t1421-reflog-write.sh`: **10/10**.
- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty -p grit-rs` ✅ (reverted unrelated edits)
  - `cargo test -p grit-lib --lib` ✅ (98/98)
