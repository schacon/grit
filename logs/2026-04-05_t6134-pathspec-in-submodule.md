## 2026-04-05 — t6134-pathspec-in-submodule

### Claim
- Marked `t6134-pathspec-in-submodule` as in progress in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t6134-pathspec-in-submodule.sh` initially showed partial status (2/3) once run in a clean harness state.
- Direct test run confirmed the remaining failure:
  - `error message for path inside submodule from within submodule`
  - Expected stderr to contain: `in unpopulated submodule`.

### Root cause
- `grit add` already had logic to reject explicit pathspecs that descend into a submodule
  (`fatal: Pathspec 'sub/a' is in submodule 'sub'`).
- But when invoked as `git -C sub add .` from within an **unpopulated submodule directory**, the
  command resolved the superproject repository and continued without the dedicated
  `in unpopulated submodule` guard that Git applies.
- As a result, `test_must_fail git -C sub add .` did not fail with the expected message.

### Fix implemented
- Updated `grit/src/commands/add.rs`:
  - After discovering repository/worktree and computing `prefix`, added a guard:
    - `die_if_in_unpopulated_submodule(&index, prefix.as_deref());`
  - Implemented `die_if_in_unpopulated_submodule`:
    - Scans stage-0 index entries for gitlinks (`mode 160000`).
    - If current `prefix` is exactly the gitlink path or nested under it, prints:
      - `fatal: in unpopulated submodule '<name>'`
    - Exits with code `128`, matching expected Git behavior for this scenario.

### Validation
- `cargo build --release` succeeded.
- `./scripts/run-tests.sh t6134-pathspec-in-submodule.sh` → **3/3 pass**.
- Direct run:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit timeout 120 bash tests/t6134-pathspec-in-submodule.sh`
  - Result: **3/3 pass**.
- Adjacent regression checks:
  - `./scripts/run-tests.sh t6136-pathspec-in-bare.sh` → **1/3** (unchanged baseline, still failing).
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **7/7 pass** (no regression).

### Tracking updates
- Marked `t6134-pathspec-in-submodule` complete in `PLAN.md` (3/3).
- Updated `progress.md` counts:
  - completed: 47
  - in progress: 0
  - remaining: 720
  - total: 767
- Updated `test-results.md` with latest validation runs.
