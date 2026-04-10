## 2026-04-10 — status phase2 partial: avoid second untracked walk in non-ignored modes

### Scope

- Implement a Phase 2 optimization from the status performance plan:
  - reuse the populated untracked cache tree for untracked output collection,
  - avoid a redundant full filesystem walk when `status` does not need ignored output.

### Code changes

1. `grit-lib/src/untracked_cache.rs`
   - Added `collect_untracked_from_cache(&UntrackedCache) -> Vec<String>`.
   - Traverses `uc.root` and returns repository-relative untracked paths.
   - Preserves cache-generated shape:
     - collapsed directory entries (`dir/`) for normal mode,
     - expanded file paths when cache was built in `-uall` mode.
   - Sorts output for deterministic status formatting.

2. `grit/src/commands/status.rs`
   - In the untracked section, when cache refresh succeeds and `ignored_mode == No`,
     collect untracked output directly from the refreshed cache.
   - Fall back to existing `collect_untracked_and_ignored(...)` walk for ignored modes
     and refresh failures.
   - Keeps existing trace2 emission path (`emit_read_directory_trace`) unchanged.

### Validation

- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib` → **166 passed, 0 failed**
- Status-focused harness:
  - `./scripts/run-tests.sh t7063-status-untracked-cache.sh` → **12/58**
  - `./scripts/run-tests.sh t7508-status.sh` → **48/126**
  - `./scripts/run-tests.sh t7060-wtstatus.sh` → **10/17**
  - `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → **8/33**

No test-count regressions were observed compared with the current baseline.
