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
- `cargo build --release`: passes (rebuild after `diff-files` copy-detection/reverse wiring changes).
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4007-rename-3.sh`: 13/13 passing.
- `./scripts/run-tests.sh t4007-rename-3.sh`: 13/13 passing; `data/file-results.tsv` refreshed.
- `./scripts/run-tests.sh t4125-apply-ws-fuzz.sh`: 2/4 passing (baseline for next in-progress Diff target).
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4125-apply-ws-fuzz.sh`: 4/4 passing after `grit apply` whitespace-fix context matching updates.
- `./scripts/run-tests.sh t4125-apply-ws-fuzz.sh`: 4/4 passing; `data/file-results.tsv` refreshed.
- `cargo build --release`: passes (rebuild after `grit apply` whitespace-fix hunk matching/writing changes).
- `cargo test -p grit-lib --lib`: passes (96/96).
- `./scripts/run-tests.sh t4131-apply-fake-ancestor.sh`: 1/3 passing (baseline for next in-progress Diff target).
- `cargo build --release`: passes (rebuild after `grit apply` whitespace-fix matching changes).
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4125-apply-ws-fuzz.sh`: 4/4 passing.
- `./scripts/run-tests.sh t4125-apply-ws-fuzz.sh`: 4/4 passing; `data/file-results.tsv` refreshed.
- `cargo test -p grit-lib --lib`: passes.
- `cargo clippy --fix --allow-dirty`: passes (unrelated auto-fixes reverted; task-only changes kept).
