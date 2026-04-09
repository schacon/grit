# t5328-commit-graph-64bit-time

## Problem

- Harness skipped t5328: `TIME_IS_64BIT` / `TIME_T_IS_64BIT` lazy prereqs were missing from `tests/test-lib.sh`.
- `test_commit` override dropped `--date`, so commits never used `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE` and generation overflow was not exercised.
- `commit_graph_write` truncated committer time to `u32`, saturated large generation offsets, and never wrote the GDO2 overflow chunk.
- Corrupt/truncated GDO2 did not fail `git log` (Git expects `commit-graph overflow generation data is too small`).

## Fix

- Added lazy prereqs and `--date` handling in `test_commit`.
- `test-tool date is64bit`: match Git (`timestamp_t` = `git_date::tm::Timestamp` / u64 size).
- Write GDA2 overflow markers + GDO2 chunk; store 64-bit committer dates in CDAT (high bits in packed word).
- `CommitGraphLayer::try_parse` validates GDA2 vs GDO2; `CommitGraphChain::try_load`; validate when `core.commitgraph` is on in `rev_list` and `log`.

## Verify

`./scripts/run-tests.sh t5328-commit-graph-64bit-time.sh` → 6/6 pass.
