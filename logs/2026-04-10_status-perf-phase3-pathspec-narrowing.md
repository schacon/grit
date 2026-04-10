## 2026-04-10 — status pathspec early narrowing (Phase 3 slice)

### Scope

- Implement early pathspec narrowing in `status` to avoid paying full-tree costs
  for `status -- <pathspec>`.
- Keep output semantics unchanged by preserving existing pathspec matcher behavior.

### Code changes

1. `grit/src/commands/status.rs`
   - Moved pathspec normalization (`user_pathspecs`) earlier in `run`.
   - Applied pathspec filtering immediately after:
     - `diff_index_to_tree(...)` (staged set),
     - `diff_index_to_worktree_with_options(...)` (unstaged set),
     before optional rename detection.
   - For untracked cache fast-path:
     - filter cache-derived untracked paths with the same matcher before formatting.
   - Threaded `pathspecs` through the fallback untracked/ignored walk:
     - `collect_untracked_and_ignored(..., pathspecs)`,
     - `visit_untracked_node(..., pathspecs)`,
     - `visit_untracked_directory(..., pathspecs)`,
     - `traditional_normal_directory_only(..., pathspecs)`.
   - Added early-prune checks in the untracked walk:
     - if neither the candidate path nor any pathspec relationship can match, skip recursion.

2. Matching behavior
   - Reused existing `status_path_matches(...)` logic for parity:
     - exact and normalized (`trim_end_matches('/')`) checks.
   - Added helper `status_pathspec_could_match_subtree(...)` for directory-prune decisions.

### Validation

- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- Status-focused harness:
  - `./scripts/run-tests.sh t7508-status.sh` → **48/126**
  - `./scripts/run-tests.sh t7060-wtstatus.sh` → **10/17**
  - `./scripts/run-tests.sh t7063-status-untracked-cache.sh` → **12/58**
  - `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → **12/33**

No regressions observed versus current branch baseline; this increment is perf-oriented
and narrows work for pathspec-scoped status invocations.
