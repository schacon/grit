# t2108-update-index-refresh-racy

**Date:** 2026-04-09

## Goal

Make `t2108-update-index-refresh-racy.sh` fully pass (racy mtime behavior for `git update-index --refresh`).

## What we found

- Ran `./scripts/run-tests.sh t2108-update-index-refresh-racy.sh`: **6/6** on branch `cursor/t2108-index-refresh-racy-4d5f`.
- No Rust changes were required: `grit/src/commands/update_index.rs` already implements Git-aligned behavior:
  - `index_file_mtime_pair` + `has_racy_timestamp` mirror `read-cache.c` (`is_racy_stat` / `has_racy_timestamp`).
  - `--refresh` skips rewriting the index when nothing changed and no entry is racy vs index read mtime (first t2108 test).
  - When racy or stat-updated, the index is written so the magic mtime on `.git/index` is cleared (remaining tests).

## Follow-up

- Refreshed `data/test-files.csv` and dashboards via `run-tests.sh`.
- Marked task complete in `PLAN.md` and updated `progress.md`.
