## 2026-04-06 — t6016-rev-list-graph-simplify-history

### Scope
- Claimed `t6016-rev-list-graph-simplify-history` as the next active Rev Machinery target.

### Baseline
- Current harness status from `data/file-results.tsv`: **2/12 passing**.
- Next step: reproduce direct + harness failures, then implement rev-list graph/simplification fixes.

### Investigation notes
- Reproduced current failures directly and in harness:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6016-rev-list-graph-simplify-history.sh` → **2/12**.
  - `./scripts/run-tests.sh t6016-rev-list-graph-simplify-history.sh` → **2/12**.
- Confirmed current root causes:
  1. `grit log --graph` currently has no graph renderer (plain subject output only).
  2. `grit log` does not accept several graph/simplification flags used by `t6016` (`--full-history`, `--simplify-merges`, `--sparse`, `--boundary`).
  3. Existing rev-list simplification behavior in `grit-lib` does not yet match Git’s graph-oriented history rewriting needs for this test.

### Attempted implementation (not kept)
- Built and wired a first-pass internal graph renderer into `grit/src/commands/log.rs`, including:
  - new graph execution path,
  - option plumbing to `grit_lib::rev_list`,
  - an ASCII graph state machine.
- Result of attempt:
  - direct `t6016` improved only to **5/12** and still failed key cases (3,5,6,7,8,9,12), with major layout/order mismatches versus expected Git graph output.
  - This was not coherent enough to keep; changes were reverted to avoid destabilizing unrelated behavior.

### Current status at end of session
- Code reverted to pre-attempt baseline for `grit/src/commands/log.rs`.
- Re-ran harness to refresh source-of-truth:
  - `./scripts/run-tests.sh t6016-rev-list-graph-simplify-history.sh` → **2/12** (unchanged baseline).
- No code changes remain staged for commit from this attempt; only tracking artifacts are updated.

### Next concrete path
- Implement a smaller, incremental graph strategy first (matching currently selected rev-list order/parents), then iteratively align with Git graph layout rules before tackling broader `t42xx` graph suites.
