## t4059-diff-submodule-not-initialized (claim/baseline)

### Claim
- Claimed next Diff target from plan: `t4059-diff-submodule-not-initialized`.
- Updated plan status from `[ ]` to `[~]`.

### Baseline
- `./scripts/run-tests.sh t4059-diff-submodule-not-initialized.sh` -> 1/8 passing.
- `bash scripts/run-upstream-tests.sh t4059-diff-submodule-not-initialized` -> 1/8 passing.

### Notes for next implementation step
- Focus area is diff behavior for uninitialized submodules (patch/raw/stat variants).
- Next actions:
  1. run direct upstream test script with `-v` to capture first concrete failing assertions.
  2. inspect `grit/src/commands/diff.rs`, `grit/src/commands/diff_index.rs`, and `grit/src/commands/diff_tree.rs` submodule rendering paths.
  3. implement missing uninitialized-submodule diff semantics and re-test upstream/local.
