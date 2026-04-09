# t3304-notes-mixed — 2026-04-08

## Problem

Harness showed `t3304-notes-mixed` at 0/6: `fast-import` rejected streams using delimited `data <<DELIM` (used throughout the test) and lacked `M ... inline`, `deleteall`, and `N` (notemodify) for notes refs.

## Fix

Extended `grit-lib` `fast_import`:

- `data <<delim>` parsing (Git fast-import delimited format).
- `M mode inline path` followed by `data`.
- `deleteall` clearing the in-memory tree.
- On `refs/notes/*`: `N inline`, `N :mark commit-ish`, `N <40-hex-blob> commit-ish` with Git-style note path fanout and final fanout rewrite; non-note paths preserved.

## Validation

- `cargo test -p grit-lib --lib` (includes new `fast_import_delimited_data_m_inline_and_note`).
- `./scripts/run-tests.sh t3304-notes-mixed.sh` → 6/6.
