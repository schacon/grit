# Test results (latest run)

Last update: 2026-03-31 with `cargo test --workspace` and `./tests/harness/run.sh` from repo root (see [README](README.md#running-tests)).

## `cargo test --workspace`

- **gust** (binary crate): 0 unit tests defined; build OK.
- **gust-lib**: 4 tests — all **passed** (`odb` ×2, `index` ×2).
- **Doc-tests** (`gust_lib`): 1 test — **passed** (`odb` module example).

## Ported shell harness (`tests/harness/run.sh`)

- **Binary:** `GUST_BIN` defaults to `target/debug/gust` (built if missing).
- **Selected scripts** (`tests/harness/selected-tests.txt`): only `t0001-init.sh` was listed at capture time.
- **t0001-init.sh:** 9 tests — **9 passed**, 0 failed.

## Notes

- Full breadth of ported tests depends on entries in `tests/harness/selected-tests.txt`. Expand that list as more scripts are ported (see `plan.md` / `tests/harness/`).
