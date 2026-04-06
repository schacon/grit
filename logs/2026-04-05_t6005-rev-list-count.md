## t6005-rev-list-count

Date: 2026-04-05

### Goal
Make `tests/t6005-rev-list-count.sh` fully pass by aligning `rev-list` option parsing/diagnostics for `--max-count` and `--skip`.

### Reproduction
- Baseline harness:
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` -> `3/6` (fails 3,4,5).
- Direct run from clean trash dir confirmed failures:
  - `--max-count`
  - `--max-count all forms`
  - `--skip`

### Root causes
1. `rev-list --skip <n>` (space-separated form) was not parsed; only `--skip=<n>` was supported.
2. Short numeric form with invalid suffix (`-1q`) fell through to “unsupported option” instead of integer parsing error.
3. Integer parse diagnostics did not contain `not an integer`, which test assertions grep for.

### Changes
- `grit/src/commands/rev_list.rs`
  - Added parsing support for detached `--skip <value>`.
  - Generalized short numeric parsing to treat any `-<something starting with digit>` as max-count input and route through integer parser (so `-1q` reports integer error).
  - Updated `parse_non_negative` error wording to:
    - `<flag>: '<value>' is not an integer`
  - Kept Git-compatible behavior where negative values are treated as unlimited (`usize::MAX` sentinel).

### Validation
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6005-rev-list-count.sh` -> **6/6**.
- Harness:
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` -> **6/6**.
- Regressions:
  - `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` -> **7/7**.
  - `./scripts/run-tests.sh t6110-rev-list-sparse.sh` -> **2/2**.

### Outcome
`t6005-rev-list-count` is now fully passing and marked complete in the plan.
