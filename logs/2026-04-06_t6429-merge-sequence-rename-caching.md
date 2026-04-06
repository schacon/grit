## 2026-04-06 — t6429-merge-sequence-rename-caching

### Scope
- Complete `t6429-merge-sequence-rename-caching.sh` by fixing remaining replay rename-cache behavior and trace2 `diffcore_rename` call counts.

### Baseline
- Direct: `GUST_BIN=/workspace/target/release/grit bash tests/t6429-merge-sequence-rename-caching.sh` → **9/11**.
- Remaining failures:
  - FAIL 6: expected 2 `diffcore_rename` calls, got 3.
  - FAIL 7: expected 3 `diffcore_rename` calls, got 4.

### Investigation
- Inspected `calls`/`trace.output` artifacts under failing trash directories.
- Reproduced failing scenarios in isolated `/tmp/case6-debug` and `/tmp/case7-debug` repositories with:
  - `GRIT_DEBUG_REPLAY=1`
  - `GIT_TRACE2_PERF=<path>`
- Confirmed upstream rename cache was being refreshed one replay step too often for paths already covered by cached directory renames.
- Also validated a neighboring sensitive scenario (`rename same file identically, then add file to old dir`) to avoid under-refreshing.

### Implementation changes
- `grit/src/commands/replay.rs`
  - refined upstream cache refresh gating:
    - `should_refresh_upstream_rename_cache(...)` now refreshes only when changed paths are not covered by cached exact/file or sufficiently-supported directory rename mappings and the path exists in base (or under a base parent).
    - removed stale-source forced refresh behavior that caused extra rename-detection runs on later commits.
  - tightened directory-coverage logic:
    - added `build_directory_rename_map_unconditional_with_counts(...)` and used support-count-aware directory coverage in `path_covered_by_cached_renames(...)`.
    - require at least 2 rename supports for directory-level cached-coverage matches to avoid over-aggressive suppression.

- `grit/src/commands/merge.rs`
  - retained replay merge wrapper/export and exact-rename guard improvements used by replay:
    - `ReplayTreeMergeResult` and `merge_trees_for_replay(...)`.
    - exact-rename detection skips sources where identical blob+mode still exists at original path.

### Validation
- Direct:
  - `cargo build --release`
  - `rm -rf /workspace/tests/trash.t6429-merge-sequence-rename-caching`
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6429-merge-sequence-rename-caching.sh` → **11/11**.
- Harness:
  - `./scripts/run-tests.sh t6429-merge-sequence-rename-caching.sh` → **11/11**.
- Targeted regressions:
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **7/7**.
  - `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh` → **2/2**.

### Outcome
- `t6429-merge-sequence-rename-caching` is now fully passing (**11/11**).
