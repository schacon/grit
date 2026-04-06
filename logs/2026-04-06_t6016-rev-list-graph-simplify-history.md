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

### 2026-04-06 incremental implementation update (latest)
- Re-applied and repaired the previously attempted `log --graph` implementation in `grit/src/commands/log.rs`:
  - fixed compilation issues (removed invalid `ObjectId::zero()` usage in graph buffer growth path),
  - enabled `log --graph` path for `--full-history`, `--sparse`, `--boundary`, and `--simplify-merges`,
  - switched graph traversal to use `grit_lib::rev_list` commit selection with explicit parent visibility rewriting.
- Updated rev-list path filtering in `grit-lib/src/rev_list.rs`:
  - path filtering now runs for path-limited traversals regardless of sparse/full-history mode,
  - merge and root commit inclusion logic now honors `full_history` and `sparse` semantics expected by `t6016` cases,
  - preserved all-ref walk stability by keeping insertion-order `all_ref_tips` output while deduplicating.
- Aligned local test harness commit timestamp behavior with upstream `test-lib.sh` intent:
  - `tests/test-lib.sh::test_commit` now executes `test_tick` in the parent shell before spawning the subshell, so dated commits (and graph ordering) are deterministic across tests.

### Validation evidence for this increment
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6016-rev-list-graph-simplify-history.sh` → **8/12** (improved from 2/12 baseline).
  - Remaining failures: 5, 8, 9, 12 (all graph-layout/state-machine fidelity issues).
- Harness:
  - `./scripts/run-tests.sh t6016-rev-list-graph-simplify-history.sh` → **8/12** (TSV updated).
- Regressions:
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` → 6/6.
  - `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → 7/7.
- Quality gates:
  - `cargo fmt`
  - `cargo clippy --fix --allow-dirty` (reverted unrelated edits)
  - `cargo test -p grit-lib --lib` (passing)

### Current remaining gap
- `t6016` is now blocked only on graph ASCII rendering fidelity in a few scenarios:
  - simplify-by-decoration branch-prune layout (extra continuation/collapse rows),
  - path-limited (`-- bar.txt`) column placement of side branch lines,
  - sparse and boundary collapse-row geometry/order.

### 2026-04-06 completion update
- Implemented final graph/history fixes in `grit/src/commands/log.rs`:
  - path-limited graph output now reorders selected commits into Git-compatible side-branch placement (`reorder_path_limited_graph_commits`), fixing `--graph -- bar.txt`.
  - sparse path-limited graph mode now expands dense-selected history along first-parent segments (`expand_sparse_path_limited_graph_history`) and forces first-parent graph edges for rendered nodes in this mode, fixing `--graph --sparse -- bar.txt`.
  - boundary graph output now orders boundary commits along the main first-parent chain before appending remaining boundaries (`order_boundary_commits_for_graph`), and includes boundary commits in the parent-visibility target set so edges from included commits (e.g. `C4 -> C3`) remain connected.
  - merge-parent simplification in graph parent visibility is now gated to simplify-by-decoration non-full-history cases only, preventing over-pruning in full-history and boundary scenarios.
- Direct validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6016-rev-list-graph-simplify-history.sh` → **12/12**.
- Harness validation:
  - `./scripts/run-tests.sh t6016-rev-list-graph-simplify-history.sh` → **12/12**.
- Regressions:
  - `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → 7/7.
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` → 6/6.
  - `./scripts/run-tests.sh t6009-rev-list-parent.sh` → 15/15.
