## Task: t4131-apply-fake-ancestor

### Claim
- Claimed after completing `t4125-apply-ws-fuzz`.
- Marked as `[~]` in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t4131-apply-fake-ancestor.sh` reports `1/3` passing (`2` left).

### Next
- Reproduce failing assertions in `tests/t4131-apply-fake-ancestor.sh`.
- Compare expected fake-ancestor index behavior with current `grit apply --build-fake-ancestor` behavior.
- Implement and validate fixes until `3/3` passes.
