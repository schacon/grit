## 2026-04-05 — t6428-merge-conflicts-sparse

### Claim
- Marked `t6428-merge-conflicts-sparse` as in progress in `PLAN.md`.

### Baseline reproduction
- Harness:
  - `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh` → **0/2**.
- Direct test with harness env:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6428-merge-conflicts-sparse.sh`
  - Initial failures:
    1. Sparse-checkout `set --no-cone README` did not remove `numerals` as expected.
    2. Merge conflict index output did not retain expected conflict entries for `git ls-files -t`.

### Root causes
1. **Non-cone sparse-checkout semantics were incorrect** in `grit/src/commands/sparse_checkout.rs`.
   - The implementation always treated patterns like directory-prefix includes.
   - In non-cone mode, sparse patterns are ordered include/exclude rules and should be evaluated with last-match-wins behavior.
2. **Conflict-stage entries were dropped before writing the index** in `grit/src/commands/merge.rs`.
   - `do_real_merge` removed stage 1/2/3 entries and kept only stage-0 plus a synthetic stage-2 fallback.
   - This broke conflict visibility for `ls-files -t` and sparse-conflict expectations.
3. **`ls-files -t` tag rendering for unmerged entries** in `grit/src/commands/ls_files.rs` used `C` instead of Git-compatible `M`.
   - During conflicts Git reports `M` for each unmerged stage line.

### Fixes applied
1. **Sparse-checkout non-cone matching**
   - Updated `apply_sparse_patterns` to read `core.sparseCheckoutCone`.
   - Implemented proper non-cone evaluation in `path_matches_sparse_patterns(...)`:
     - ordered pattern processing,
     - `!` negation support,
     - root-anchor normalization,
     - last-match-wins include state.
2. **Preserve full conflict index**
   - Removed the post-merge stage-collapsing block in `do_real_merge`.
   - Now writes `merge_result.index` directly so stage 1/2/3 entries survive conflicts.
3. **Align `ls-files -t` conflict tag**
   - Updated `status_tag` to return `M` for `entry.stage() != 0`.

### Validation
- `cargo build --release` after each code change.
- Target test:
  - `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh` → **2/2 pass**.
  - direct run from stable cwd (`/workspace/tests`) also **2/2 pass**.
- Regression checks:
  - `./scripts/run-tests.sh t6134-pathspec-in-submodule.sh` → **3/3 pass**.
  - `./scripts/run-tests.sh t6136-pathspec-in-bare.sh` → **3/3 pass**.
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **7/7 pass**.

### Tracking updates
- Marked `t6428-merge-conflicts-sparse` complete in `PLAN.md` (**2/2**).
- Updated `progress.md` counts and recently-completed list.
- Updated `test-results.md` with t6428 and regression evidence.
