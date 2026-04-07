# TESTING.md — Grit Test Strategy

## Overview

Grit uses the upstream Git test suite as ground truth. Harness files live under `tests/` (ported from `git/t/`) and run through `scripts/run-tests.sh` with the grit binary aliased as `git`.

The **single source of truth** for per-file status is `data/test-files.csv`. Dashboards (`docs/index.html`, `docs/testfiles.html`) are generated from that file.

## Running tests

```bash
cargo build --release -p grit-rs

# Single file
./scripts/run-tests.sh t3200-branch.sh

# One group (e.g. all t1xxx files that are not manually skipped)
./scripts/run-tests.sh t1

# Full suite (all in-scope files)
./scripts/run-tests.sh

# Options (can appear before or after the target)
./scripts/run-tests.sh --timeout 180 t0000-basic.sh
./scripts/run-tests.sh t0000-basic.sh --quiet
```

Skipped files: set `in_scope` to `skip` on that row in `data/test-files.csv`. Those files are **never** executed for single-file, group, or full runs. Their tests are **excluded** from dashboard totals on the main page.

## Data pipeline

1. **`scripts/generate-test-files-catalog.py`** — Scans `tests/t*.sh`, counts `test_expect_success` / `test_expect_failure` per file, assigns `group` (`t0`…`t9`), and writes or merges **`data/test-files.csv`**. Run this when you add or remove harness files (also run automatically at the start of `run-tests.sh`).

2. **`scripts/run-tests.sh`** — Runs the selected files, appends results to a temp batch, then **`scripts/apply-test-run-results.py`** merges into `test-files.csv` and runs **`scripts/generate-dashboard-from-test-files.py`**.

3. **`data/test-files.csv`** columns:

| Column | Meaning |
|--------|---------|
| `file` | Base name (no `.sh`) |
| `group` | `t0` … `t9` from the file prefix |
| `in_scope` | `yes` or `skip` (manual) |
| `tests_total` | Count of test markers in the file |
| `passed_last` | Pass count from the last run |
| `failing` | Fail count from the last run |
| `fully_passing` | `true` if `tests_total > 0` and `failing == 0` |
| `status` | `ok`, `timeout`, or `error` from the harness |
| `expect_failure` | Count of `test_expect_failure` lines |

## Work strategy: one file at a time

1. Pick a test file that is not fully passing.
2. Run it: `./scripts/run-tests.sh t1234-foo.sh`
3. Fix Rust in `grit/` / `grit-lib/`.
4. Re-run until green; `test-files.csv` updates automatically.

### Priority order

1. Plumbing (`t0xxx`, `t1xxx`)
2. Index/checkout (`t2xxx`)
3. Core commands (`t3xxx`)
4. Diff (`t4xxx`)
5. Transport (`t5xxx`)
6. Rev machinery (`t6xxx`)
7. Porcelain (`t7xxx`)
8. External helpers (`t9xxx`) last

## test_expect_failure

When you fix known breakage, flip `test_expect_failure` → `test_expect_success` in the test file.

## test-lib.sh

**Do not** modify `tests/test-lib.sh` casually — past changes caused regressions.

## Dashboards

Regenerated automatically after every `run-tests.sh` run. To refresh HTML only (no test run):

```bash
python3 scripts/generate-dashboard-from-test-files.py
```
