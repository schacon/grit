# Test results (latest run)

Last update: 2026-03-31 (Phase 2.1/2.2/2.3).

## Phase 2 symbolic-ref/show-ref validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell scripts

- `tests/t1401-symbolic-ref.sh` -> PASS (10/10)
- `tests/t1403-show-ref.sh` -> PASS (7/7)
- `tests/t1422-show-ref-exists.sh` -> PASS (8/8)

## Rev-parse task validation (2026-03-31)

### Commands executed

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS
- `GUST_BIN=target/debug/gust TEST_VERBOSE=1 sh tests/t1500-rev-parse.sh` -> PASS (`8/8`)
- `GUST_BIN=target/debug/gust TEST_VERBOSE=1 sh tests/t1503-rev-parse-verify.sh` -> PASS (`8/8`)

### Notes

- `t1503-rev-parse-verify.sh` intentionally exercises failing `--verify` cases; stderr includes `Needed a single revision` for those negative checks while the script result is PASS.

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

---

## Consolidated report (2026-03-31)

### Commands executed

- `cargo test --workspace`
- `./tests/harness/run-all.sh` (verified pass)
- Per-script accounting sweep over `tests/t*.sh` for exact totals

### Rust workspace totals

- **Result:** PASS
- **Individual tests:** 5
  - `gust` unit tests: 0
  - `gust-lib` unit tests: 4
  - `gust-lib` doc-tests: 1
- **Failures:** 0

### Shell suite totals (`tests/t*.sh`)

- **Result:** PASS
- **Scripts run:** 26
- **Individual tests:** 139
- **Pass:** 139
- **Fail:** 0
- **Skip:** 0

### Overall totals (Rust + shell)

- **Individual tests run:** 144
- **Pass:** 144
- **Fail:** 0
- **Skip:** 0

### Areas covered

- Init/repo basics: `t0000`, `t0001`
- Object plumbing: `t1006`, `t1007`, `t1100`
- Read-tree family: `t1000`, `t1001`, `t1002`, `t1003`, `t1005`, `t1008`, `t1009`
- Subdirectory behavior: `t1020`
- Refs: `t1400`, `t1404`
- Checkout-index: `t2002`–`t2006`
- Update-index / ls-files: `t2107`, `t3004`
- Ls-tree / quoting: `t3100`, `t3102`, `t3902`
- Write-tree: `t5000`

---

## Task 0.1 validation (2026-03-31)

### Commands executed

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS

### Rust workspace totals

- **Result:** PASS
- **gust** (binary crate): 0 tests run, 0 failed.
- **gust-lib**: 4 tests run, 4 passed, 0 failed.
- **Doc-tests** (`gust_lib`): 1 test run, 1 passed, 0 failed.

### Shell harness

- `./tests/harness/run.sh` was not run in this task (scope limited to CLI registration/stubs).
