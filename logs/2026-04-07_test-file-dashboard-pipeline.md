# 2026-04-07 — Test-file-based data pipeline

## What changed

- **Source of truth:** `data/test-files.csv` (replaces `file-results.tsv`, `test-results.tsv`, `command-status.tsv`, `git-test-cases.tsv`).
- **Catalog:** `scripts/generate-test-files-catalog.py` scans `tests/t*.sh`, sets `group` (t0–t9), counts test markers, default `in_scope=yes`.
- **Runner:** `scripts/run-tests.sh` supports single file, group (`t1`), or full suite; rows with `in_scope=skip` are never run; after each run, `scripts/apply-test-run-results.py` merges batch results and runs `scripts/generate-dashboard-from-test-files.py`.
- **Dashboards:** `docs/index.html` — summary cards (excluding skipped files/tests) + clickable group cards with progress; `docs/testfiles.html` — filterable table, `?group=t1` from index links.

## Removed

- Old TSV pipeline scripts under `scripts/subscripts/` and `generate-testfiles-html.py`, `update-dashboard.sh`, `timeline.sh`, `build-dashboard.py`.
- Large generated docs: `tests.html`, `timeline.html`, `progress.svg`.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t0004-unwritable.sh`
