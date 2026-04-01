# Test results (latest run)

Last update: 2026-03-31 with `cargo test --workspace` and `./tests/harness/run.sh` from repo root (see [README](README.md#running-tests)).

## `cargo test --workspace`

- **Result:** PASS
- **gust** (binary crate): 0 tests run, 0 failed.
- **gust-lib**: 4 tests run, 4 passed, 0 failed.
- **Doc-tests** (`gust_lib`): 1 test run, 1 passed, 0 failed.

## Ported shell harness (`tests/harness/run.sh`)

- **Binary:** `GUST_BIN` defaults to `target/debug/gust` (built if missing).
- **Selected scripts** (`tests/harness/selected-tests.txt`): `t0001-init.sh`, `t1100-commit-tree-options.sh`.
- **Result:** PASS
- **t0001-init.sh:** 9 tests — 9 passed, 0 failed.
- **t1100-commit-tree-options.sh:** 5 tests — 5 passed, 0 failed.

## Notes

- Full breadth of ported tests depends on entries in `tests/harness/selected-tests.txt`. Expand that list as more scripts are ported (see `plan.md` / `tests/harness/`).
