## t6404-recursive-merge — completion log (2026-04-05)

### Goal
Make `tests/t6404-recursive-merge.sh` fully pass without test stubbing.

### Reproduction baseline
- `./scripts/run-tests.sh t6404-recursive-merge.sh` -> `4/6`.
- Direct run failures:
  - **test 4** `virtual trees were processed` (stage-1 OID mismatch in criss-cross merge).
  - **test 5** `refuse to merge binary files` (missing Git-style binary conflict message).

### Root causes
1. **Virtual merge-base conflict materialization mismatch**
   - `create_virtual_merge_base(...)` synthesized stage-0 fallback from `conflict_files`, but in criss-cross nested conflicts the exact stage-1 blob identity must be preserved from conflict stages (Git uses nested temporary-branch conflict marker content).
   - Result: final merge’s stage-1 OID differed from expected `idxstage1`.

2. **Binary add/add conflicts had generic output only**
   - `try_content_merge_add_add(...)` treated binary add/add as generic conflict content without a specialized diagnostic, so expected:
     - `Cannot merge binary files: binary-file (HEAD vs. F)`
     was missing.

3. **`rm --cached` symlink recursion edge**
   - `rm` used `Path::is_dir()` in recursion checks; this follows symlink targets and incorrectly rejected tracked symlink paths as directories (`not removing ... recursively without -r`) in `t6415` scenarios.

### Implemented fixes
1. **Virtual base stage fidelity**
   - `grit/src/commands/merge.rs` (`create_virtual_merge_base`):
     - Keep deterministic ordering by parsed commit timestamp for criss-cross base folding.
     - Reuse stage entries directly for conflicted paths:
       - prefer existing stage-0
       - else synthesize stage-0 from stage-1/2/3 entries and ODB (preserves Git-compatible conflict blob identity)
       - fall back to `conflict_files` only when no staged candidate exists.
     - This aligned stage-1 OIDs with expected virtual-tree processing in test 4.

2. **Binary conflict diagnostics**
   - `grit/src/commands/merge.rs`:
     - Extended `ContentMergeResult` with `BinaryConflict`.
     - `try_content_merge` and `try_content_merge_add_add` now return `BinaryConflict` for binary unresolved cases.
     - Merge callers map binary conflicts to `conflict_descriptions` type `binary`.
     - Conflict output printer emits:
       - `Cannot merge binary files: <path> (HEAD vs. <branch>)`
     - Satisfies test 5 assertion in `t6404`.

3. **`rm --cached` symlink-safe directory checks**
   - `grit/src/commands/rm.rs`:
     - Replaced `is_dir()` checks with `symlink_metadata(...).file_type().is_dir()` where recursion checks/removal dispatch are performed.
     - Prevents symlink-target-following false positives on recursion requirement.

### Validation
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6404-recursive-merge.sh` -> **6/6**.
- Harness:
  - `./scripts/run-tests.sh t6404-recursive-merge.sh` -> **6/6**.
- Regression checks:
  - `./scripts/run-tests.sh t6415-merge-dir-to-symlink.sh` -> 24/24.
  - `./scripts/run-tests.sh t6421-merge-partial-clone.sh` -> 3/3.
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` -> 7/7.
  - `./scripts/run-tests.sh t3600-rm.sh` -> 49/82 (snapshot only; no new claim).

### Result
- `t6404-recursive-merge` is now fully passing and can be marked complete.
