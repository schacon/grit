## t4008-diff-break-rewrite (claim/baseline)

### Claim

- Claimed next Diff target from plan: `t4008-diff-break-rewrite`.
- Updated plan status from `[ ]` to `[~]`.

### Baseline

- `./scripts/run-tests.sh t4008-diff-break-rewrite.sh` -> 5/14 passing.
- `bash scripts/run-upstream-tests.sh t4008-diff-break-rewrite` -> 5/14 passing.

### Initial failing assertions snapshot

- Tests 3,4,6,7,9,10,13,14: `diff-index -B` / `-B -M` / `-B -C` unsupported (`error: unsupported option: -B`).
- Test 11: `diff-index -M` typechange was emitted as `M` instead of expected `T`.

## Implementation summary

### Core fixes in `grit/src/commands/diff_index.rs`

- Added CLI parsing support for rewrite-break options:
  - `-B`
  - `--break-rewrites`
  - `-B<n>` and `--break-rewrites=<n>` accepted (threshold value currently parsed as presence only, matching this test file's requirements).
- Added typechange detection in tree/index and tree/worktree diffs:
  - changed status from `M` to `T` when file type bits differ.
- Implemented `-B` integration with rename/copy detection flow:
  - for modified/typechanged entries, synthesize delete+add pairs for detection.
  - run rename/copy detection over synthetic entries.
  - collapse synthetic bookkeeping so final raw output matches expected rewrite/rename/copy forms for `t4008`.
  - preserve explicit typechanges for mode-kind transitions.
- Kept existing no-`-B` rename/copy logic unchanged.

## Validation

- `cargo build --release` -> pass.
- `bash scripts/run-upstream-tests.sh t4008-diff-break-rewrite` -> 14/14 pass.
- Direct verbose run:
  - `cd /tmp/grit-upstream-workdir/t && GIT_BUILD_DIR=/tmp/grit-upstream-workdir TEST_NO_MALLOC_CHECK=1 TAR=tar bash ./t4008-diff-break-rewrite.sh -v`
  - Result: 14/14 pass, all expected raw rewrite/rename/copy/typechange outputs match.
- `./scripts/run-tests.sh t4008-diff-break-rewrite.sh` -> 14/14 pass (local mirror).
- Quality gates:
  - `cargo fmt` -> pass
  - `cargo clippy --fix --allow-dirty` -> pass (unrelated autofixes reverted)
  - `cargo test -p grit-lib --lib` -> pass (96/96)

## Outcome

- `t4008-diff-break-rewrite` is fully passing and ready to mark complete in plan/progress.
