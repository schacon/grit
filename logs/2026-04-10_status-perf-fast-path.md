## 2026-04-10 — status tracked-file fast path (racy-aware)

### Scope

- Implement Phase 1 tracked-file performance optimization from the status performance plan:
  - add racy-aware stat-trust fast path for index→worktree status diffing,
  - thread index mtime context from `status` into diffing.

### Code changes

1. `grit-lib/src/diff.rs`
   - Added `DiffIndexToWorktreeOptions` with optional `index_mtime: Option<(u32, u32)>`.
   - Added `diff_index_to_worktree_with_options(...)`.
   - Kept existing `diff_index_to_worktree(...)` as compatibility wrapper using default options.
   - Added `entry_is_racy(...)` helper implementing Git-style racy check:
     - entry considered racy when `entry.mtime >= index_mtime`.
   - In `diff_index_to_worktree_with_options`:
     - compute `stat_same = stat_matches(...)`,
     - preserve mode-only change reporting,
     - skip hashing when `stat_same && mode_same && !racy`,
     - hash only uncertain/racy entries.

2. `grit/src/commands/status.rs`
   - Added local `index_file_mtime_pair(...)` helper.
   - Sample index mtime from `repo.index_path()` before loading/diffing.
   - Switched unstaged diff call to `diff_index_to_worktree_with_options(...)`,
     passing `DiffIndexToWorktreeOptions { index_mtime: Some(index_mtime) }`.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`
- `cargo test -p grit-lib --lib` → **166 passed, 0 failed**

Status-focused harness run in `/tmp/grit-status-perf` (binary copied from current branch):

- `./scripts/run-tests.sh t7508-status.sh` → **48/126**
- `./scripts/run-tests.sh t7060-wtstatus.sh` → **10/17**
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh` → **12/58**
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → **8/33**

No regressions were observed versus the baseline captured at the start of this effort; this change is performance-focused and does not yet address untracked-cache/fsmonitor parity gaps.
