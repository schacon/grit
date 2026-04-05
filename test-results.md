# Test Results

**Updated:** 2026-04-05

- `cargo test --workspace`: not run for this task.
- `./tests/harness/run.sh`: not run for this task.
- `cargo build --release -p grit-rs`: success.
- `./scripts/run-tests.sh t0050-filesystem.sh`: 13/13 passing.
- `./scripts/run-tests.sh t3102-ls-tree-wildcards.sh`: 4/4 passing.
- `./scripts/run-tests.sh t3500-cherry.sh`: 4/4 passing.
- `./scripts/run-tests.sh t3009-ls-files-others-nonsubmodule.sh`: 2/2 passing.
- `./scripts/run-tests.sh t3908-stash-in-worktree.sh`: 2/2 passing.
- `cargo fmt && cargo clippy --fix --allow-dirty && cargo test -p grit-lib --lib`: success.
- `cargo build --release -p grit-rs`: success (rebuilt after test-tool changes).
- `./scripts/run-tests.sh t3008-ls-files-lazy-init-name-hash.sh`: 1/1 passing.
- `./scripts/run-tests.sh t3205-branch-color.sh`: 4/4 passing.
- `./scripts/run-tests.sh t1601-index-bogus.sh`: 4/4 passing.
- `./scripts/run-tests.sh t3012-ls-files-dedup.sh`: 3/3 passing.
