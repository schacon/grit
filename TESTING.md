# TESTING.md — Grit Test Strategy

## Overview

Grit uses the upstream Git test suite as ground truth. The 1,011 upstream test files
(from `git/t/`) have been ported to `tests/` with a `test-lib.sh` shim that wraps
the grit binary as `git`. The goal: make every upstream test file pass 100%.

## Running Tests

All test runs go through `scripts/run-tests.sh`, which caches results in
`data/file-results.tsv`. Both `docs/index.html` and `docs/testfiles.html` read
from this same TSV.

```bash
# Single file
./scripts/run-tests.sh t3200-branch.sh

# Category (e.g., all t1xxx plumbing tests)
./scripts/run-tests.sh t1

# Full suite
./scripts/run-tests.sh

# Only re-run files that aren't fully passing
./scripts/run-tests.sh --failing

# Only run files with no cached results
./scripts/run-tests.sh --stale

# Options
./scripts/run-tests.sh --timeout 180    # per-file timeout (default 120s)
./scripts/run-tests.sh --force          # re-run everything
./scripts/run-tests.sh --quiet          # minimal output
```

## Data Files

All in `data/`:

| File | Purpose |
|------|---------|
| `git-test-cases.tsv` | Master catalog of all 18,097 upstream test cases (extracted from `git/t/`) |
| `file-results.tsv` | Per-file test results (updated by `run-tests.sh`) — **single source of truth** |
| `test-results.tsv` | Per-test-case status (derived from file-results) |
| `command-status.tsv` | Per-command aggregate stats (derived from test-results) |

**Pipeline:** `run-tests.sh` → `file-results.tsv` → `extract-and-test.py` → `test-results.tsv` + `command-status.tsv` → `generate-progress-html.py` → `docs/index.html`

For `docs/testfiles.html`: `file-results.tsv` → `generate-testfiles-html.py`

## Work Strategy: One File at a Time

Pick a single test file that isn't fully passing. Make it fully pass. Commit. Move on.

### Priority Order

Work through files in this order:

1. **Plumbing (t0xxx, t1xxx)** — Core infrastructure, everything builds on these
2. **Index/Checkout (t2xxx)** — Working tree operations
3. **Core commands (t3xxx)** — ls-files, merge, cherry-pick, rm, add, mv
4. **Diff (t4xxx)** — Diff engine, format-patch, log
5. **Transport (t5xxx)** — Pack, fetch, push, clone
6. **Rev machinery (t6xxx)** — Rev-list, rev-parse, merge-base, for-each-ref
7. **Porcelain (t7xxx)** — commit, status, tag, branch, reset, grep
8. **External helpers (t9xxx)** — p4, svn, cvs, completion (lowest priority)

Within each category, prefer files with **fewer remaining failures** (closer to
fully passing = quicker wins).

### Workflow

```bash
# 1. Pick a file
./scripts/run-tests.sh --failing     # see what's still broken

# 2. Run it and study failures
cd tests && GUST_BIN="$(pwd)/grit" bash t1234-foo.sh

# 3. Fix the Rust code in grit/ or grit-lib/
#    (fix GRIT, not the tests — unless flipping test_expect_failure)

# 4. Rebuild and re-test
cd /path/to/grit && cargo build --release
cd tests && GUST_BIN="$(pwd)/grit" bash t1234-foo.sh

# 5. Once fully passing, update results
./scripts/run-tests.sh t1234-foo.sh

# 6. Commit
git add -A && git commit -m "fix: make t1234-foo fully pass"

# 7. Regen dashboards
python3 scripts/subscripts/extract-and-test.py
python3 scripts/generate-testfiles-html.py
python3 scripts/subscripts/generate-progress-html.py
git add data/ docs/ && git commit -m "docs: update dashboards"
git push origin main
```

## test_expect_failure

These are tests marked as "known breakage" — they run the test body and expect
it to *fail*. When you fix the Rust code so the test body passes:

1. Flip `test_expect_failure` → `test_expect_success` in the test file
2. The test now passes normally

**Semantics:** In the test output:
- `ok N # TODO known breakage` = body FAILED (expected) — still broken
- `not ok N # TODO known breakage` = body SUCCEEDED — flip it!

## test-lib.sh

**DO NOT** modify test-lib.sh to remove subshell isolation or add auto git-init.
Past changes caused massive regressions. The BIN_DIRECTORY lives outside the
working tree to survive `git clean -x`.

## Dashboards

Regenerate after merging fixes:

```bash
# Update all dashboards from file-results.tsv
python3 scripts/subscripts/extract-and-test.py    # updates test-results.tsv + command-status.tsv
python3 scripts/generate-testfiles-html.py         # docs/testfiles.html
python3 scripts/subscripts/generate-progress-html.py  # docs/index.html
```
