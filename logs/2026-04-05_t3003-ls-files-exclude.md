# t3003-ls-files-exclude

- Date: 2026-04-05 19:52 CEST
- Result: 7/7 upstream tests passing

## What changed

- No `ls-files` source fix was required in this turn: `HEAD` already passes upstream `t3003-ls-files-exclude`.
- Updated the stale tracking entry in `PLAN.md` and recorded the completion in `progress.md`.
- Added `#[allow(unused_mut)]` to `get_process_ancestry()` in `grit/src/main.rs` to keep `cargo check -p grit-rs` warning-free on non-Linux builds while preserving the Linux mutation.

## Verification

- Read `AGENTS.md`, the `t3003-ls-files-exclude` entry in `PLAN.md`, and upstream `git/t/t3003-ls-files-exclude.sh`
- Ran `cargo build --release`
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t3003 bash scripts/run-upstream-tests.sh t3003`
- Confirmed `Tests: 7 (pass: 7, fail: 0)`
- Ran `cargo fmt`
- Ran `cargo check -p grit-rs 2>&1 | rg warning` and confirmed no warnings
- Ran `cargo test -p grit-lib --lib`

## Notes

- `CARGO_TARGET_DIR=/tmp/grit-build-t3003 cargo clippy --fix --allow-dirty` could not run in this sandbox because Cargo failed to bind its TCP listener for lock management (`Operation not permitted`).
