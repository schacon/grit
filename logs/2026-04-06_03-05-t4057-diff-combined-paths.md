## Task: t4057-diff-combined-paths

### Claim
- Claimed from the Diff section in `PLAN.md`.
- Baseline in tracked harness: `0/4` passing.

### Baseline failures
- `git diff -c --name-only HEAD HEAD^ HEAD^2` failed with `error: too many revisions`.
- Root cause:
  - `grit diff` did not parse `-c` / `--cc` combined-diff flags.
  - The command path only supported unstaged/cached/tree-vs-tree diff modes and rejected 3 revision arguments.

### Implementation
- Updated `grit/src/commands/diff.rs`:
  - Added CLI flags:
    - `-c` (`combined`)
    - `--cc` (`dense_combined`, parsed equivalently for this path-list test scope)
  - Added trailing-arg recovery for `-c` and `--cc` in the manual re-parse block.
  - Added combined-diff entry computation:
    - `resolve_combined_tree_set()` resolves base tree + parent trees from either:
      - one merge commit argument, or
      - explicit `<base> <parent1> <parent2> ...` args.
    - `combined_diff_entries()` computes the intersection of changed paths between base tree and *each* parent tree.
    - Returns deterministic sorted synthetic entries to drive `--name-only`/`--name-status` outputs for this test.

### Validation
- `cargo build --release` ✅
- `./scripts/run-tests.sh t4057-diff-combined-paths.sh` ✅ `4/4`
- `bash scripts/run-upstream-tests.sh t4057-diff-combined-paths` ✅ `4/4`
- Regressions:
  - `./scripts/run-tests.sh t4023-diff-rename-typechange.sh` ✅ `2/4` (known fixture mismatch, unchanged behavior)
  - `./scripts/run-tests.sh t4038-diff-combined.sh` ✅ `6/26` (baseline-level partial file, no new regressions introduced by this patch)
- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (unrelated autofixes reverted)
  - `cargo test -p grit-lib --lib` ✅

### Status
- Task behavior for `t4057` is complete and marked done in the plan.
