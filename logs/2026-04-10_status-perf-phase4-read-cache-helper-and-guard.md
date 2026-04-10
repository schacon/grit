## 2026-04-10 — fsmonitor parity increment (test-tool read-cache + empty fsmonitor guard)

### Scope

- Files changed:
  - `grit/src/main.rs`
  - `tests/test-tool`
  - `grit/src/commands/update_index.rs`
- Goal: advance `t7519-status-fsmonitor.sh` parity by addressing remaining test-tool and fsmonitor-refresh edge cases.

### Code changes

1. Added `test-tool read-cache` implementation in `grit`:
   - New dispatcher branch in `main` (`"read-cache" => run_test_tool_read_cache(rest)`).
   - New helper `run_test_tool_read_cache` supporting:
     - `--print-and-refresh=<path>`
     - optional numeric loop count argument
   - Behavior:
     - performs quiet refresh (`update_index::run_refresh_quiet`) before each probe;
     - prints `<path> is up to date` / `<path> is not up to date` from index fsmonitor-valid state;
     - writes the loop counter to the target path between iterations.

2. Wired shell wrapper support for `test-tool read-cache`:
   - In `tests/test-tool`, replaced no-op `read-cache` stub with delegation to the `grit` test-tool subcommand.

3. Hardened fsmonitor refresh query guard:
   - In `update_index::query_fsmonitor_paths`, treat empty `core.fsmonitor` config as disabled (`None`) to avoid trying to execute an empty path.

### Validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: passed (reverted unrelated edits)
- `cargo test -p grit-lib --lib`: 166 passed
- `bash tests/t7519-status-fsmonitor.sh -v`: 31/33 (remaining fails: 33 only in some runs; stable remaining set now narrowed to `31`/`33` while iterating, then `33` in full-flow repro)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 26/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58
- `./scripts/run-tests.sh t7508-status.sh`: 94/126
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28

### Notes

- `t7519.30` is now passing reliably after prior untracked-cache dedup change.
- `t7519.31` behavior is much closer with read-cache helper wired, but still environment/sequence sensitive when run in isolated subsets versus full flow.
- `t7519.33` remains failing in isolated runs due to sparse-checkout content expectation mismatch (`cp full/dir1 ...`), requiring further sparse-index/status parity work.
