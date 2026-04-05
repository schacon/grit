## Task: t4044-diff-index-unique-abbrev

### Claim
- Claimed after completing `t4217-log-limit`.
- Marked as `[~]` in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t4044-diff-index-unique-abbrev.sh` reports `0/2` passing (`2` left).

### Next
- Reproduce failing assertions in `tests/t4044-diff-index-unique-abbrev.sh`.
- Investigate unique abbreviation behavior in `grit diff-index --abbrev`.
- Implement and validate fixes until `2/2` passes.
