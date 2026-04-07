# TESTING.md — Grit Test Strategy

## Overview

Grit uses the upstream Git test suite as ground truth. Harness files live under `tests/` (ported from `git/t/`) and run through `scripts/run-tests.sh` with the grit binary copied into `tests/grit` and exposed as `git` via the harness.

The **single source of truth** for per-file harness status is **`data/test-files.csv`**. There are no intermediate TSVs (no `file-results.tsv`, no per-command aggregates). After each run, **`docs/index.html`** (summary + progress by group) and **`docs/testfiles.html`** (per-file table, filterable by group) are regenerated from that CSV.

## Running tests

Build the binary first; the runner expects **`target/release/grit`**.

```bash
cargo build --release -p grit-rs

# Single file
./scripts/run-tests.sh t3200-branch.sh

# Prefix run: `tests/<arg>*.sh` (e.g. `t1` matches all `t1*.sh` harness files)
./scripts/run-tests.sh t1

# Full suite (every row in test-files.csv with in_scope=yes)
./scripts/run-tests.sh

# Options (can appear before or after the target)
./scripts/run-tests.sh --timeout 180 t0000-basic.sh
./scripts/run-tests.sh t0000-basic.sh --quiet
```

### Manually skipped files

Edit **`data/test-files.csv`**: set **`in_scope`** to **`skip`** on the row for that file. Skipped files are **never** executed (single-file, group, or full run). Their tests are **excluded** from the summary counts on **`docs/index.html`**. They still appear on **`docs/testfiles.html`** with a skipped badge so you can see what was opted out.

Re-run **`python3 scripts/generate-test-files-catalog.py`** if you add or rename `.sh` files and want the CSV updated without running tests (otherwise the next `run-tests.sh` also refreshes the catalog).

## Pipeline diagram

```
generate-test-files-catalog.py     (start of run-tests.sh: discover files, groups, marker counts)
            │
            ▼
    data/test-files.csv  ◄────  apply-test-run-results.py  ◄────  run-tests.sh (bash harness per file)
            │
            └──►  generate-dashboard-from-test-files.py  ──►  docs/index.html
                                                           ──►  docs/testfiles.html
```

## Scripts reference

| Script | Role |
|--------|------|
| `scripts/generate-test-files-catalog.py` | Scan `tests/t*.sh`, merge **`data/test-files.csv`** (preserves `in_scope` and prior run columns where possible). |
| `scripts/run-tests.sh` | Select files to run, execute harness, invoke apply + dashboard. |
| `scripts/apply-test-run-results.py` | Merge one batch of run lines into **`data/test-files.csv`**, then call the dashboard generator. |
| `scripts/generate-dashboard-from-test-files.py` | Read CSV only; write **`docs/index.html`** and **`docs/testfiles.html`**. |

## Data pipeline (step by step)

1. **`scripts/generate-test-files-catalog.py`** — Scans `tests/t*.sh`, counts `test_expect_success` / `test_expect_failure` per file, assigns `group` (`t0`–`t9` from the first digit of the `tNNNN…` prefix, matching **`git/t/README`** test families), and writes or merges **`data/test-files.csv`**. Invoked automatically at the start of **`run-tests.sh`**.

2. **`scripts/run-tests.sh`** — Copies `target/release/grit` to `tests/grit`, builds the file list (honoring **`in_scope`**), runs each selected script under `timeout`, parses the `# Tests:` summary line, writes a small batch TSV for **`scripts/apply-test-run-results.py`**.

3. **`scripts/apply-test-run-results.py`** — Updates matching rows in **`data/test-files.csv`** (`passed_last`, `failing`, `fully_passing`, `status`, etc.), then runs **`scripts/generate-dashboard-from-test-files.py`**.

4. **`data/test-files.csv`** columns:

| Column | Meaning |
|--------|---------|
| `file` | Base name (no `.sh`) |
| `group` | `t0`–`t9` (first digit of `tNNNN…`; see `git/t/README` and catalog script) |
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

## Other runners (not the CSV pipeline)

These do **not** update `data/test-files.csv` by default:

- **`scripts/run-upstream-tests.sh`** / **`scripts/aggregate-upstream.sh`** — run upstream `git/t/` against grit in isolation (see **AGENTS.md**).
- **`tests/harness/run-all-count.sh`** — separate harness; not wired to `test-files.csv`.
