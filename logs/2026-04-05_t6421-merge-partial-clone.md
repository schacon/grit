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

### 2026-04-05 follow-up progress

#### Additional fixes implemented
- `grit-lib/src/rev_list.rs`
  - added `MissingAction` enum and `RevListOptions.missing_action`.
  - object collection now handles missing referenced objects according to mode:
    - `Error`: fail immediately (existing behavior)
    - `Print`: continue and collect missing OIDs for output
    - `Allow`: continue silently
  - added `RevListResult.missing_objects`.
- `grit/src/commands/rev_list.rs`
  - parses `--missing=<mode>` with support for:
    - `--missing=error`
    - `--missing=print`
    - `--missing=allow-any`
    - `--missing=allow-promisor`
  - when mode is `print`, outputs missing objects as `?<oid>` lines.
- `grit/src/commands/merge.rs`
  - added compatibility parsing for:
    - `--progress`
    - `--no-progress`
  - options are accepted as no-ops for now (sufficient for argument compatibility).
- `grit/src/commands/pull.rs`
  - wired new merge args (`progress`, `no_progress`) through `pull`’s merge invocation to keep build green.

#### Validation run results
- `cargo build --release -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty` ✅ (with unrelated edits reverted)
- `cargo test -p grit-lib --lib` ✅ (96/96)
- `./scripts/run-tests.sh t6421-merge-partial-clone.sh` ❌ still `0/3`
  - current remaining failures are behavioral:
    - expected trace2 `fetch_count` events are absent
    - partial-clone lazy-fetch behavior not implemented
    - third test still hits merge conflict behavior mismatch for this scenario
- Regression checks:
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` ✅ 7/7
  - `./scripts/run-tests.sh t8200-mv-rename.sh` ✅ 30/30

#### Updated status
- `t6421` remains in progress (`0/3`).
- Option-parsing blockers (`--missing=print`, `--no-progress`) are resolved.
- Remaining work is deeper partial-clone fetch/trace behavior and merge parity.
