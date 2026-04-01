# Test results (latest run)

Last update: 2026-03-31 (v2 completion wave: rev-parse prefix + repack + gc).

## Final v2 completion validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)
- `./tests/harness/run.sh` -> PASS

### Newly added scripts in this wave

- `tests/t1513-rev-parse-prefix.sh` -> PASS (`11/11`)
- `tests/t7700-repack.sh` -> PASS (`4/4`)
- `tests/t6500-gc.sh` -> PASS (`4/4`)

### Full harness totals (selected scripts)

- Scripts run: **51**
- Individual shell tests: **282**
- Pass: **282**
- Fail: **0**
- Skip: **0**

### Overall totals (Rust + shell)

- Rust tests (`cargo test --workspace`): **5/5 PASS**
- Shell tests (`./tests/harness/run.sh`): **282/282 PASS**
- Combined individual tests: **287 PASS**, **0 FAIL**

## Phase 5 rev-list validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell scripts

- `tests/t6000-rev-list-misc.sh` -> PASS (`4/4`)
- `tests/t6003-rev-list-topo-order.sh` -> PASS (`4/4`)
- `tests/t6005-rev-list-count.sh` -> PASS (`5/5`)
- `tests/t6006-rev-list-format.sh` -> PASS (`4/4`)
- `tests/t6014-rev-list-all.sh` -> PASS (`2/2`)
- `tests/t6017-rev-list-stdin.sh` -> PASS (`4/4`)

### Notes

- Implemented and validated a coherent `rev-list` subset covering range parsing (`^A`, `A..B`), `--all`, `--stdin`, `--first-parent`, `--ancestry-path`, `--simplify-by-decoration`, ordering (`--topo-order`, `--date-order`, `--reverse`), limits (`--max-count`, `-n`, `--skip`, `--count`), and formatting (`--parents`, `--format`, `--quiet`).
- Deferred rev-list functionality remains tracked in `plan.md` item **5.4** (bitmap/promisor behavior).

## Phase 6 diff-index validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell scripts

- `tests/t4013-diff-various.sh` -> PASS (`4/4`)
- `tests/t4017-diff-retval.sh` -> PASS (`5/5`)
- `tests/t4044-diff-index-unique-abbrev.sh` -> PASS (`3/3`)

### Notes

- Implemented `gust diff-index` for selected Phase 6 subset: raw output, `--cached`, non-cached missing-file handling with `-m`, `--quiet`/`--exit-code`, `--abbrev[=<n>]`, and basic pathspec filtering.
- `--patch` (`-p`) is intentionally not implemented in this subset because the selected scripts do not require patch output.

## Phase 7 for-each-ref validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell scripts

- `tests/t6300-for-each-ref.sh` -> PASS (`6/6`)
- `tests/t6301-for-each-ref-errors.sh` -> PASS (`5/5`)
- `tests/t6302-for-each-ref-filter.sh` -> PASS (`7/7`)

### Notes

- Implemented a coherent `for-each-ref` subset covering sorting, patterns, excludes, count limiting, stdin pattern input, covered format atoms (`refname`, `refname:short`, `objectname`, `objecttype`, `subject`, `*subject`), and filters (`--points-at`, `--merged`, `--no-merged`, `--contains`, `--no-contains`).
- Added error-path parity for broken loose refs and zero-OID refs (warnings) plus missing object behavior (fatal for object-dependent formats, non-fatal for objectname-only output).

## Phase 8 count-objects/verify-pack validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell scripts

- `tests/t5301-sliding-window.sh` -> PASS (`3/3`)
- `tests/t5304-prune.sh` -> PASS (`2/2`)
- `tests/t5613-info-alternate.sh` -> PASS (`3/3`)

### Harness status

- `./tests/harness/run.sh` -> FAIL due to pre-existing `for-each-ref` suite failures (`tests/t6300-for-each-ref.sh`: 3 failing cases). This is outside Phase 8 scope; newly added Phase 8 scripts were run directly and pass.

## Phase 3 check-ignore validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell script

- `tests/t0008-ignores.sh` -> PASS (12/12)

### Notes

- Ported a coherent `t0008` subset focused on `check-ignore` path arguments, `--stdin` / `-z`, `-v` / `-n`, `--no-index`, and precedence across `.gitignore`, `.git/info/exclude`, and `core.excludesfile`.

## Phase 4 merge-base validation (2026-03-31)

### Required validation commands

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)

### Newly ported shell script

- `tests/t6010-merge-base.sh` -> PASS (`10/10`)

### Notes

- `tests/t6010-merge-base.sh` covers default merge-base behavior, `--all`, `--octopus`, `--independent`, `--is-ancestor`, and corner cases for disjoint histories and repeated commits.
- While validating this task, pre-existing lint/build issues in `ignore` and `check-ignore` paths surfaced and were fixed to satisfy the required workspace `clippy -D warnings` gate.

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
