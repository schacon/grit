# t6426-merge-skip-unneeded-updates

Date: 2026-04-06

## Baseline

- `./scripts/run-tests.sh t6426-merge-skip-unneeded-updates.sh` was at **1/13**.
- Direct run (`GUST_BIN=/workspace/target/release/grit bash tests/t6426-merge-skip-unneeded-updates.sh`) showed failures across:
  - stderr cleanliness on successful merges,
  - `test-tool chmtime --get -3600 <path>` helper behavior,
  - unnecessary worktree rewrites when merged index content was unchanged,
  - rename/add conflict staging/content edge case (`2c`),
  - and replay side-effects discovered during regression checks.

## Implemented fixes

### 1) `tests/test-tool` chmtime compatibility

- Reworked `chmtime` parsing to match upstream helper behavior:
  - supports `--get/-g` and `--verbose/-v`,
  - supports optional timespec argument with forms:
    - `<seconds>`, `+<seconds>`, `-<seconds>`
    - `=<seconds>`, `=+<seconds>`, `=-<seconds>`
  - supports `--get <offset> <file>` patterns used by `t6426`.

### 2) Merge output channels

- Updated merge success commit summary lines (`[branch sha] subject`) to stdout (`println!`) instead of stderr.
- Kept conflict diagnostics on stdout (matching script expectations that `err` is empty for successful merges).

### 3) Skip unneeded worktree updates

- Added optional old-index map support to merge checkout:
  - `checkout_entries(..., old_entries_for_skip)` now skips writing a path when:
    - old stage-0 OID/mode == new stage-0 OID/mode, and
    - the worktree path already exists and is not a directory/symlink mismatch.
- Wired this in merge paths:
  - normal recursive/ort merge uses pre-merge `ours_entries`,
  - octopus merge uses current tree snapshot.
- This prevents unnecessary mtime bumps for unchanged content in cases like 1a-L, 2b-L, 4a, 4b.

### 4) Rename/add conflict correctness (case 2c)

- In rename passes, ensure rename/add conflict handling:
  - removes any stale stage-0 entry for the conflict path before staging conflict entries,
  - computes conflict-file content via `try_content_merge_add_add(...)` so worktree file has conflict markers,
  - preserves expected stage orientation for `rename/add`:
    - stage 2 = existing path on ours side,
    - stage 3 = renamed-content side used by test expectations.
- Added cleanup of stale source-path snapshot entries when both sides perform identical rename/rename(1to1), avoiding false add-source carryover into later passes.

### 5) Replay regression fix discovered during verification

- While validating merge regressions, `t6429` regressed due to forced directory-rename handling in replay merges.
- Introduced explicit merge directory-rename mode plumbing in merge core and set replay to `Disabled`:
  - `MergeDirectoryRenamesMode::{FromConfig, Enabled, Disabled}`
  - standard merge/merge-recursive keep `FromConfig`,
  - replay uses `Disabled` to preserve expected rename-cache semantics in `t6429`.

## Validation

- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6426-merge-skip-unneeded-updates.sh` → **13/13**
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6429-merge-sequence-rename-caching.sh` → **11/11**
- Harness:
  - `./scripts/run-tests.sh t6426-merge-skip-unneeded-updates.sh` → **13/13**
  - `./scripts/run-tests.sh t6429-merge-sequence-rename-caching.sh` → **11/11**
  - `./scripts/run-tests.sh t6406-merge-attr.sh` → **13/13**
  - `./scripts/run-tests.sh t6432-merge-recursive-space-options.sh` → **11/11**

## Notes

- Ran quality gates:
  - `cargo fmt`
  - `cargo clippy --fix --allow-dirty`
  - `cargo test -p grit-lib --lib`
- Reverted unrelated clippy/dashboard churn before finalizing commit scope.
