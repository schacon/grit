## t4042-diff-textconv-caching (claim/baseline)

### Claim
- Claimed next Diff target from plan: `t4042-diff-textconv-caching`.
- Updated plan status from `[ ]` to `[~]`.

### Baseline
- `./scripts/run-tests.sh t4042-diff-textconv-caching.sh` -> 1/8 passing.

### Notes for next implementation step
- Focus area is textconv caching behavior in diff output paths.
- Next actions:
  1. run direct test script with `TEST_VERBOSE=1` to capture first concrete failing assertions.
  2. inspect `grit/src/commands/diff.rs` textconv execution/caching paths and shared helpers in `grit-lib`.
  3. implement minimal caching semantics to align with upstream expectations and re-test.
