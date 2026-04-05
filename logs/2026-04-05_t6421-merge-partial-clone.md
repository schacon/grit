## t6421-merge-partial-clone

Date: 2026-04-05

### Claim
- Marked `t6421-merge-partial-clone` as in-progress (`[~]`) in the plan.

### Reproduction
- `./scripts/run-tests.sh t6421-merge-partial-clone.sh` initially showed `0/3`.
- Direct traced run (`bash -x tests/t6421-merge-partial-clone.sh`) showed setup failing early:
  - `git switch B-single` aborted with:
    - `error: Your local changes to the following files would be overwritten by checkout: ...`
  - This happened immediately after commit `A` creation in `test_setup_repo`.

### Root cause (first blocker)
- In `grit mv`, after rename operations we were refreshing index stat metadata from the moved worktree files.
- That made moved entries appear unstaged-clean (`index == worktree`) and `git commit` did not include those renames/content updates in commit `A`.
- As a result, branch `A` had missing expected changes and subsequent branch switches in the test setup observed unexpected dirty files.

### Fix (first blocker)
- Updated `grit/src/commands/mv.rs`:
  - Removed post-rename stat-refresh for moved index entries.
  - Kept index metadata as staged state so renamed paths remain correctly committed by subsequent `git commit`.

### Validation (first blocker)
- Local targeted reproduction script now succeeds:
  - commit `A` created
  - `git switch B-single` succeeds
  - no dirty-state abort
- Regression checks:
  - `./scripts/run-tests.sh t8200-mv-rename.sh` → `30/30`
  - `./scripts/run-tests.sh t12120-mv-verbose-dryrun.sh` → `33/33`

### Additional compatibility progress
- `t6421` also invoked unsupported clone/merge flags:
  - `git clone --sparse ...` (unsupported before)
  - Added `--sparse` acceptance to clone args:
    - `grit/src/commands/clone.rs` (`Args.sparse: bool`)
- Re-ran `t6421`; it progressed further but still fails due to unrelated missing functionality:
  - `rev-list --missing=print` unsupported
  - `merge --no-progress` unsupported
  - no partial-clone lazy-fetch/trace2 `fetch_count` behavior yet

### Current status
- `t6421` remains in progress (`0/3`), but the setup blocker from `mv` is fixed.
- Next work for full pass requires implementing missing `rev-list` and merge option support plus partial-clone object fetching semantics.
