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

## Follow-up increment — alternate index + check_only subtree parity

### Additional change summary

1. **Status honors `GIT_INDEX_FILE` for read/write**
   - `grit/src/commands/status.rs` now resolves index path with `repo.index_path_for_env()?`
   - This fixes `iuc` helper parity in `t7063` where status must read/write the temporary index.

2. **Porcelain v1 branch-header auto-enable removed**
   - Removed status-only auto-injection of `args.branch = true` for porcelain v1.
   - Porcelain branch header now appears only when explicitly requested (`-b`) or already set by existing option/config parsing, matching upstream expectations used by `t7063`.

3. **UNTR check_only tree materialization for collapsed directories**
   - `grit-lib/src/untracked_cache.rs` now materializes collapsed-directory children as
     in-memory `check_only` subtrees using already collected `sub_untracked` paths (no extra
     `read_dir`).
   - Reuses valid `check_only` subtrees on subsequent runs for collapsed dirs.
   - Preserves expected `dump-untracked-cache` structure while keeping trace `opendir` counters aligned.

4. **UNTR parse + collection consistency fixes**
   - Parsed UNTR nodes now restore `recurse=true` baseline before bitmap application.
   - `collect_untracked_from_cache` ignores `check_only` directories to avoid leaking hidden nested entries in normal mode.

5. **FSMonitor untracked-shape regression fix**
   - Removed unconditional top-level-only pruning of untracked paths under fsmonitor+untracked-cache.
   - Restores direct `t7519` parity for compare-status tests while retaining reported-path filtering.

### Follow-up validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7063-status-untracked-cache.sh --run=1-7 -v`: **7/7**
- `bash tests/t7519-status-fsmonitor.sh -v`: **33/33**
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: **24/58** (from 14/58)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 27/33 (no regression)

## Follow-up increment — UNTR mode-bypass and invalidation parity refinement

### Additional change summary

1. **Status-side UNTR bypass parity for explicit `-u*` overrides**
   - `grit/src/commands/status.rs` now bypasses untracked-cache refresh only when
     explicit CLI `-u*` conflicts with cache mode and cache mode matches current config,
     aligning with upstream mode-switch behavior in `t7063`.
   - When bypassing, trace emission now writes only `....path:` (no zero counters), matching
     expected `get_relevant_traces` output.

2. **Cache traversal excludes `check_only` nodes from recursion output**
   - `grit-lib/src/untracked_cache.rs` cache-source traversal now filters recursive children
     with `d.recurse && !d.check_only`, preventing hidden check-only subtrees from being
     walked into visible untracked output.

3. **UNTR `show_all` expansion parity**
   - For `-uall` refreshes, directory handling now goes directly through
     `read_directory_recursive(..., show_all=true)` to produce full file-level entries and
     expected trace-open counts.

4. **Per-directory `.gitignore` change invalidation and OID persistence**
   - Added per-directory ignore-file OID tracking in `read_directory_recursive`.
   - On OID changes for valid nodes, mark gitignore invalidation and clear cached untracked
     entries recursively for that node, matching trace and dump expectations around test 17+.
   - Persist directory exclude OIDs (with root/special handling) so UNTR dump lines match
     expected root/exclude hash shape in `t7063` fixtures.

5. **Invalidation preserves check-only placeholders**
   - `invalidate_directory()` no longer clears `recurse` bit on `check_only` children, preserving
     expected serialized placeholder nodes across index path invalidations (tests 20+ shape checks).

### Follow-up validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7063-status-untracked-cache.sh --run=1-20 -v`: **passes through 19/20; only test 20 remains failing**
- `bash tests/t7519-status-fsmonitor.sh -v`: **33/33** (no regression)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: **34/58** (from 24/58)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 27/33 (no regression)
- `./scripts/run-tests.sh t7508-status.sh`: 94/126 (no regression)
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (no regression)
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28 (no regression)

