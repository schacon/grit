# Test Results

**Updated:** 2026-04-05

- `cargo build --release`: passes (rebuild after diff trailing-stat option parsing fix).
- `bash scripts/run-upstream-tests.sh t4073-diff-stat-name-width`: 6/6 passing.
- `./scripts/run-tests.sh t4073-diff-stat-name-width.sh`: 6/6 passing; `data/file-results.tsv` refreshed.
- `cargo fmt`: passes.
- `cargo clippy --fix --allow-dirty`: passes (some unrelated auto-fixes were produced and then reverted; only task-related code kept).
- `cargo test -p grit-lib --lib`: passes.
- `./scripts/run-tests.sh t4006-diff-mode.sh`: 6/7 passing (improved from 5/7 after `update-index --chmod` worktree mode sync).
- `bash scripts/run-upstream-tests.sh t4006-diff-mode`: 7/7 passing in isolated upstream harness.
- `cargo test --workspace`: not run for this task.
- `./tests/harness/run.sh`: not run for this task.
- `CARGO_TARGET_DIR=/tmp/grit-build-t1303 bash scripts/run-upstream-tests.sh t1303`: 11/11 passing after rebuilding `target/release/grit`.
- `bash scripts/run-upstream-tests.sh t4006-diff-mode`: 7/7 passing (verified after mode/stat fixes).
- `./scripts/run-tests.sh t4006-diff-mode.sh`: 7/7 passing; `data/file-results.tsv` refreshed.
