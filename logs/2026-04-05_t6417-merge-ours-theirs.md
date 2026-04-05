## 2026-04-05 — t6417-merge-ours-theirs

### Claim
- Marked `t6417-merge-ours-theirs` as in progress in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` currently reports **6/7**.
- The missing case is likely one of:
  - binary + `-Xours/-Xtheirs`,
  - pull propagation of `-X`,
  - or SYMLINKS-specific merge preference behavior.

### Planned execution
- Reproduce with direct run:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6417-merge-ours-theirs.sh`
- Identify exact failing assertion.
- Patch merge option/favor propagation in:
  - `grit/src/commands/merge.rs` (strategy-option parsing/application),
  - and/or pull passthrough behavior if failure is in `git pull ... -X...`.
- Re-run:
  - direct file test,
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh`,
  - one adjacent merge suite for regression guard.

### Reproduction findings
- Direct run failed at SYMLINKS test in branch creation/switch sequence:
  - checkout refused with:
    - `error: The following untracked working tree files would be overwritten by checkout: link`
- Root cause #1:
  - `commit` pathspec staging canonicalized symlink paths and stripped against canonicalized worktree,
    causing staged entry `link` to be written into index as `file`.
  - This broke later checkout safety logic by making `link` appear untracked.
- After fixing root cause #1, a second failure remained:
  - `git diff --exit-code two HEAD -- link` reported `error: too many revisions`.
- Root cause #2:
  - `diff` revision/path disambiguation used `Path::exists()` only; in symlink scenarios (`link`
    path may not exist due to checkout/remove/replace timing), pathspec was misclassified as revision.

### Fixes applied
1. `grit/src/commands/commit.rs`
   - In `stage_pathspec_files`, switched to lexical path resolution relative to CWD instead of
     canonicalizing each path, preserving symlink filenames correctly.
   - Keeps canonicalized worktree root for robust `strip_prefix`, but no longer dereferences target
     path symlinks.
2. `grit/src/commands/diff.rs`
   - In `parse_rev_and_paths`, treat args as paths when `symlink_metadata(arg).is_ok()` as well as
     `Path::exists()`.
   - This matches git behavior for symlink pathspecs that should be treated as paths even when
     `exists()` semantics are unreliable for the specific invocation context.

### Validation results
- `GUST_BIN=/workspace/target/release/grit bash tests/t6417-merge-ours-theirs.sh` → **7/7 pass**.
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **7/7 pass**.
- Regression checks:
  - `./scripts/run-tests.sh t6414-merge-rename-nocruft.sh` → **3/3 pass**.
  - `./scripts/run-tests.sh t6408-merge-up-to-date.sh` → **7/7 pass**.

### Tracking updates
- Marked `t6417-merge-ours-theirs` complete in `PLAN.md` (7/7).
- Updated `progress.md` counts:
  - completed: 43
  - in progress: 0
  - remaining: 724
  - total: 767
