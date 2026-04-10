# t4050-diff-histogram

## Goal

Make `tests/t4050-diff-histogram.sh` pass (10/10).

## Changes

1. **Git-compatible histogram line diff** — Added `imara-diff` (workspace dep) and wired `histogram` separately from `patience` in `grit diff`. Histogram uses imara-diff with `postprocess_lines` and unified output; patience/minimal/myers still use `similar`. `--no-index` uses histogram hunks from raw file text when no whitespace rules apply (matches Git).

2. **Bare repo `rev:path`** — `normalize_colon_path_for_tree` in `rev_parse.rs` now resolves simple paths in bare repos without a work tree (rejects `./` / `../` like Git).

3. **`--stat --no-index`** — Per-file line uses `format_stat_line` so the `+`/`-` bar matches Git (t4050 `expect_diffstat`).

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t4050-diff-histogram.sh` → 10/10
