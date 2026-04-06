## 2026-04-06 — t6009-rev-list-parent

### Scope
- Worked `t6009-rev-list-parent.sh` to completion while `t6016` remains in progress.
- Goal: restore missing rev-list option compatibility and revision-token parsing expected by upstream test flow.

### Baseline
- Direct run before changes:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6009-rev-list-parent.sh` → **12/15**
  - Failures:
    - unsupported `--not` option (tests 2 and 15)
    - unsupported `--no-max-parents` option (test 13)
- Harness status before changes:
  - `./scripts/run-tests.sh t6009-rev-list-parent.sh` showed partial (3/15 in stale tracking).

### Root causes found
1. `grit rev-list` argument parser accepted `--stdin` token toggling via library helper, but did **not** accept command-line `--not` in normal argument flow.
2. Parser supported `--min-parents` / `--max-parents`, but not explicit reset flags:
   - `--no-min-parents`
   - `--no-max-parents`
3. Revision token `rev^!` (single-commit range syntax) was not expanded; parser passed it directly to rev-parse and failed with `object not found: t1^!`.

### Implementation
- File: `grit/src/commands/rev_list.rs`

1. Added support for option toggles in argument parser:
   - `--not` now flips a local `cli_not_mode` boolean and inverts polarity of subsequently collected revision specs in CLI mode.
   - `--no-min-parents` now clears `options.min_parents`.
   - `--no-max-parents` now clears `options.max_parents`.

2. Added `rev^!` expansion handling before stdin merge:
   - For any revision token ending in `^!`:
     - add `<rev>` as positive
     - add `^<rev>^` as negative
   - This reproduces the expected single-commit include/exclude behavior used in `t6009` test 15.

3. Kept all existing symmetric-diff (`A...B`) handling intact.

### Validation
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6009-rev-list-parent.sh` → **15/15**
- Harness:
  - `./scripts/run-tests.sh t6009-rev-list-parent.sh` → **15/15**
- Targeted regressions:
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` → 6/6
  - `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → 7/7
  - `./scripts/run-tests.sh t6016-rev-list-graph-simplify-history.sh` → 8/12 (unchanged; still active)

### Current status
- `t6009-rev-list-parent` is now fully passing and ready to mark complete in tracking docs.
- `t6016` remains the active in-progress graph/layout task at 8/12.
