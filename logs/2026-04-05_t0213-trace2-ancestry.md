## t0213-trace2-ancestry

- Confirmed the requested upstream reproduction command from `/Users/schacon/projects/grit` was stale there because `scripts/run-upstream-tests.sh` hardcoded `/home/hasi/grit`.
- Fixed this worktree's upstream runner to resolve the repo from the script location, honor a single test-file filter, use portable `find`, and fall back when `timeout` is unavailable.
- Updated the injected upstream `test-tool` shim so `test-tool trace2 ...` delegates to `grit test-tool trace2 ...`.
- Extended `grit test-tool trace2` to support the helper verbs needed by `t0213`: `001return`, `004child`, and `400ancestry`.
- Emitted `cmd_ancestry` for trace2 `perf` and `event` targets in addition to `normal`; the JSON target now writes an `ancestry` array so the upstream filter matches it.
- Manually removed one pre-existing `unused_mut` warning in `grit-lib/src/rev_list.rs` because `cargo clippy --fix --allow-dirty` is blocked in this sandbox by `failed to bind TCP listener to manage locking (os error 1)`.

## Verification

- `CARGO_TARGET_DIR=/tmp/grit-build-trace2 bash scripts/run-upstream-tests.sh t0213`
  - Result: 5 tests, 5 pass, 0 fail.
- `cargo check -p grit-rs`
  - Result: pass.
- `cargo test -p grit-lib --lib`
  - Result: 95 passed, 0 failed.
- `cargo fmt`
  - Result: pass.
- `cargo clippy --fix --allow-dirty`
  - Result: blocked by sandbox TCP-listener restriction, not by code errors.
