## 2026-04-10 — status ignore pruning for excluded directories

### Scope

- Continue Phase 3 status performance work by adding directory-level pruning for
  fully excluded directories in default ignored mode (`--ignored=no`).
- Apply the optimization to both:
  - plain status untracked/ignored walk (`grit/src/commands/status.rs`),
  - untracked-cache-backed walk paths (`grit-lib/src/untracked_cache.rs`).

### Code changes

1. `grit/src/commands/status.rs`
   - In `visit_untracked_directory(...)`, added an early return:
     - if `ignored_mode == IgnoredMode::No`,
     - and `matcher.check_path(..., rel, true)` reports the directory excluded,
     - then skip recursion immediately.
   - This runs after tracked-descendant checks, preserving tracked-path behavior.

2. `grit-lib/src/untracked_cache.rs`
   - In `visit_untracked_directory_uc(...)` and
     `visit_untracked_directory_collect(...)`, added equivalent early prune when
     `ignored_mode == UntrackedIgnoredMode::No` and the directory itself is excluded.
   - Keeps UNTR refresh/collection behavior aligned with the non-cache walk.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib` → 166 passed
- Harness:
  - `./scripts/run-tests.sh t0008-ignores.sh` → 219/398
  - `./scripts/run-tests.sh t7067-status-untracked-dir.sh` → 32/33
  - `./scripts/run-tests.sh t7063-status-untracked-cache.sh` → 12/58
  - `./scripts/run-tests.sh t7508-status.sh` → 48/126

No functional regressions were observed in the targeted status-related suites; this increment
focuses on pruning unnecessary recursion in excluded trees.
