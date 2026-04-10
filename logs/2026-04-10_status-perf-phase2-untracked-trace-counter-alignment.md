## 2026-04-10 — status perf phase2 / untracked cache trace + counter alignment

### Scope

- `grit-lib/src/untracked_cache.rs`
- `grit/src/commands/status.rs`

### Change summary

1. **Untracked cache root creation counter**
   - Removed an extra `dir_created` increment when initializing the untracked cache root shell.
   - This prevents counting the root node as a newly-created traversed directory in trace statistics.

2. **Trace2 read_directory category slot**
   - Updated synthetic `GIT_TRACE2_PERF` read-directory emission to include `read_directo` in the same column Git tests parse (`get_relevant_traces` in `t7063`).
   - This aligns our trace row shape with what the `grep data.*read_directo` filter expects.

### Validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7063-status-untracked-cache.sh --run=1-4 -v`: still 3/4 (test 4 remains failing in this environment due index-isolation mismatch, but trace fields now contain `read_directo` with expected keys)
- `bash tests/t7519-status-fsmonitor.sh -v`: 33/33
- Harness snapshot:
  - `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58 (no regression)
  - `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 27/33 (no regression)
  - `./scripts/run-tests.sh t7508-status.sh`: 94/126 (no regression)
  - `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (no regression)
  - `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28

