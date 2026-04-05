# Test Results

**Updated:** 2026-04-05

- `CARGO_TARGET_DIR=/tmp/grit-build-t1601 bash scripts/run-upstream-tests.sh t1601`: 4/4 passing against `target/release/grit`; the `plan.md` entry was stale.
- `cargo fmt`: completed for the `t1601` verification pass.
- `cargo clippy --fix --allow-dirty`: blocked in this sandbox because Cargo failed to bind a TCP listener for locking (`Operation not permitted`).
- `CARGO_TARGET_DIR=/tmp/grit-build-t1503 bash scripts/run-upstream-tests.sh t1503`: 12/12 passing against `target/release/grit`.
- `cargo fmt`: completed for the `t1503` changes.
- `CARGO_TARGET_DIR=/tmp/grit-build-t1503 cargo clippy --fix --allow-dirty`: blocked in this sandbox because Cargo failed to bind a TCP listener for locking (`Operation not permitted`).
- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 bash scripts/run-upstream-tests.sh t3102`: 4/4 passing against `target/release/grit`.
- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 cargo fmt`: completed.
- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 cargo clippy --fix --allow-dirty`: blocked in this sandbox because Cargo failed to bind a TCP listener for locking (`Operation not permitted`).
- `CARGO_TARGET_DIR=/tmp/grit-build-t3500 cargo test -p grit-lib --lib`: 96/96 passing.
- `./tests/harness/run.sh`: not run for this task.
- `CARGO_TARGET_DIR=/tmp/grit-build-t3500 bash scripts/run-upstream-tests.sh t3500`: 4/4 passing against `target/release/grit`; the `plan.md` entry was stale.
- `CARGO_TARGET_DIR=/tmp/grit-build-t3500 cargo clippy --fix --allow-dirty`: blocked in this sandbox because Cargo failed to bind a TCP listener for locking (`Operation not permitted`).
- `CARGO_TARGET_DIR=/tmp/grit-build-t3205 bash scripts/run-upstream-tests.sh t3205`: 4/4 passing after rebuilding `target/release/grit`; the `plan.md` entry was stale.
- `CARGO_TARGET_DIR=/tmp/grit-build-t3205 cargo clippy --fix --allow-dirty`: blocked in this sandbox because Cargo failed to bind a TCP listener for locking (`Operation not permitted`).
- `cargo build --release -p grit-rs`: completed to refresh `target/release/grit`, which the upstream runner uses.
- `CARGO_TARGET_DIR=/tmp/grit-build-t1408 bash scripts/run-upstream-tests.sh t1408`: 3/3 passing against rebuilt `target/release/grit`; the `PLAN.md` entry was stale.
- `cargo fmt`: completed.
- `CARGO_TARGET_DIR=/tmp/grit-build-t1408 cargo clippy --fix --allow-dirty`: blocked in this sandbox because Cargo failed to bind a TCP listener for locking (`Operation not permitted`).
