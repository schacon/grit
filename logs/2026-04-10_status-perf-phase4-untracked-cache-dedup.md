## 2026-04-10 — untracked-cache dedup for repeated status walks

### Scope

- File changed: `grit-lib/src/untracked_cache.rs`
- Goal: remove duplicate untracked-cache entries that surfaced in fsmonitor/UNTR flows.

### Change summary

- In the cache-backed directory source path (`DirSource::Cache`) we now skip cached
  `child_files` entries that are already collapsed directory markers (`name/`).
- Previously these markers could be re-traversed as real directory entries on
  subsequent status runs, causing duplicate `dir1/` / `dir2/` lines in
  `test-tool dump-untracked-cache` output and failing fsmonitor UNTR invalidation parity.

### Why

- `t7519.30` expects UNTR stability when fsmonitor reports `.git`-only changes.
- Re-consuming collapsed markers from cache source caused repeated synthetic
  entries to accumulate across runs.

### Validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7519-status-fsmonitor.sh -v`: improved from 30/33 to 31/33 (remaining failures: 31, 33)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: improved to 25/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58 (no regression)
- `./scripts/run-tests.sh t7508-status.sh`: 94/126 (no regression)
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (no regression)
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28
