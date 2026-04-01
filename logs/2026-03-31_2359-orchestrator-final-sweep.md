# Orchestrator final sweep (2026-03-31)

- Re-ran and fixed regressions in `cat-file`, `hash-object`, `commit-tree` and test harness helpers.
- Implemented remaining `read-tree` phase work and ported `t1000`, `t1001`, `t1002`, `t1003`, `t1005`, `t1008`, `t1009`.
- Fixed failing suites for `update-ref`, `checkout-index`, `ls-tree` quoting, and `write-tree` missing-object behavior.
- Ported integration scripts `t1020-subdirectory.sh` and `t0000-basic.sh` subset.
- Expanded `tests/harness/selected-tests.txt` to include all currently ported `t*.sh` scripts.
- Verified with `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `./tests/harness/run.sh`.
