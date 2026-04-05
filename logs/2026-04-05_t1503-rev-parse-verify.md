# t1503-rev-parse-verify

- Date: 2026-04-05 19:42 Europe/Berlin
- Result: 12/12 upstream tests passing

## What changed

- Added `--rewrite` as an accepted compatibility flag for `grit reflog delete`, which unblocked the deleted-reflog setup path used by `t1503`.
- Extended reflog approxidate parsing in `rev-parse` to handle relative selectors like `1.year.ago`, allowing `--verify -q` date-based reflog lookups to resolve correctly.

## Verification

- `cargo fmt`
- `CARGO_TARGET_DIR=/tmp/grit-build-t1503 cargo build --release -p grit-rs`
- `CARGO_TARGET_DIR=/tmp/grit-build-t1503 bash scripts/run-upstream-tests.sh t1503`
- `CARGO_TARGET_DIR=/tmp/grit-build-t1503 bash scripts/run-upstream-tests.sh t1503 2>&1 | tail -40`

## Notes

- `cargo fmt` completed.
- `cargo clippy --fix --allow-dirty` could not run in this sandbox because Cargo attempted to bind a TCP listener for lock management and the OS denied it (`Operation not permitted`).
- `scripts/run-tests.sh` could not be used here because the environment only provides Bash 3.2, while the script requires associative arrays.
