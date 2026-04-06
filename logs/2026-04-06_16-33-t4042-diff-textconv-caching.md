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

## Implementation and validation (completed)

### Code changes
- Implemented full textconv spec resolution in `grit diff` with:
  - `diff.<driver>.textconv`
  - `diff.<driver>.cachetextconv`
  - respect for `--no-textconv`
- Added notes-backed textconv cache support under:
  - `refs/notes/textconv/<driver>`
  - note payload format: `program <cmd>\n\n<converted-bytes>`
  - cache invalidation on command change by matching `program ...` header
- Wired binary-patch textconv path to:
  - prefer cached conversion when enabled
  - write cache notes on cache misses
- Ensured textconv attribute lookup always uses repository worktree attribute context when available (for commit-vs-commit diffs), fixing `.gitattributes` application in `git diff HEAD^ HEAD`.
- Extended no-index attribute/config loading to include `core.attributesFile` rules even outside a repository.
- Enabled textconv for `--no-index` patch rendering when a textconv driver is configured (including `core.attributesFile` + `-c diff.<driver>.textconv=...` cases).

### Test evidence
- `bash scripts/run-upstream-tests.sh t4042-diff-textconv-caching` → **8/8 pass**.
- Direct upstream verbose execution:
  - `cd /tmp/grit-upstream-workdir/t && ... bash ./t4042-diff-textconv-caching.sh -v`
  - all 8 tests pass, including:
    - first/cached textconv output
    - helper-not-run cache hit behavior
    - cache invalidation on textconv command change
    - `git log --no-walk -p refs/notes/textconv/magic HEAD`
    - no-index + `core.attributesFile` outside repo case.
- Local mirror run:
  - `./scripts/run-tests.sh t4042-diff-textconv-caching.sh` currently reports **7/8**
  - known harness divergence: simplified local `nongit` helper keeps unexpected cwd semantics, causing the final no-index path lookup to run outside expected directory.
  - authoritative upstream harness is fully green.
