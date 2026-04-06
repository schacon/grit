## t2018-checkout-branch — 2026-04-05

- Claimed next near-complete target after `t2202`: `t2018-checkout-branch` (21/25).
- Reproduced failures with:
  - `./scripts/run-tests.sh t2018-checkout-branch.sh` → 21/25
  - `GUST_BIN=/workspace/tests/grit TEST_VERBOSE=1 bash tests/t2018-checkout-branch.sh`

- Root causes identified:
  1. Branch-name resolution regression: `checkout -b @{-1}` created a literal branch named `@{-1}` in some flows instead of resolving to the previous branch name before existence checks and error reporting.
  2. `clone --no-checkout` + `checkout <rev> -b <name>` did not always populate worktree/index when HEAD already matched target commit (initial checkout population missing).
  3. Sparse-checkout interaction: initial population helper incorrectly forced worktree materialization in sparse mode, breaking the “preserves mergeable changes despite sparse-checkout” scenario.
  4. Error text compatibility:
     - `checkout -b existing` needed exact `fatal: a branch named ... already exists` wording.
     - `checkout -b ... <path>` needed `Cannot update paths and switch to branch ...` wording.
  5. Test harness compatibility issue in this environment:
     - stale `.git/info` directory could persist from previous runs; this made `mkdir .git/info` fail in test 23.
     - Added idempotent cleanup in test harness setup to match upstream behavior expectations in repeated local runs.

- Implemented fixes:
  - `grit/src/commands/checkout.rs`
    - Ensured inline and normal `-b/-B` flows resolve `@{-N}` before branch-create checks.
    - Updated branch-exists error path to emit exact Git-compatible `fatal:` message.
    - Added robust `checkout <start> -b/-B <name>` compatibility parsing/dispatch.
    - Refined initial-population logic:
      - populate for empty index or no materialized tracked files in non-sparse mode;
      - skip forced population when `core.sparseCheckout=true`.
    - Added path/update rejection wording compatibility for extra path arguments.
  - `tests/test-lib.sh`
    - In `setup_trash()`, remove stale `.git/info` directory before each test script run to keep repeated local execution deterministic.

- Validation:
  - `cargo fmt` ✅
  - `cargo build --release -p grit-rs` ✅
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t2018-checkout-branch.sh` ✅ (25/25)
  - `./scripts/run-tests.sh t2018-checkout-branch.sh` ✅ (25/25)
  - `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated churn files afterward)
  - `cargo test -p grit-lib --lib` ✅ (96/96)
