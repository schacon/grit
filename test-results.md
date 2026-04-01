# Test results (latest run)

Last update: 2026-03-31.

## `cargo test --workspace`

- **Result:** PASS
- **gust** (binary crate): 0 tests run, 0 failed.
- **gust-lib**: 4 tests run, 4 passed, 0 failed.
- **Doc-tests** (`gust_lib`): 1 test run, 1 passed, 0 failed.

## Ported shell tests (`tests/t*.sh`)

- **Result:** PASS for every current ported script (`26/26`).
- Passing scripts include:
  - `t0000-basic.sh`, `t0001-init.sh`
  - `t1000`/`t1001`/`t1002`/`t1003`/`t1005`/`t1006`/`t1007`/`t1008`/`t1009`
  - `t1020-subdirectory.sh`, `t1100-commit-tree-options.sh`
  - `t1400-update-ref.sh`, `t1404-update-ref-errors.sh`
  - `t2002`/`t2003`/`t2004`/`t2005`/`t2006`
  - `t2107-update-index-basic.sh`
  - `t3004-ls-files-basic.sh`
  - `t3100-ls-tree-restrict.sh`, `t3102-ls-tree-wildcards.sh`
  - `t3902-quoted-ls-tree.sh`
  - `t5000-write-tree.sh`

## Harness selection

`tests/harness/selected-tests.txt` now lists all currently ported `t*.sh` scripts so `./tests/harness/run.sh` executes the full local ported suite.
