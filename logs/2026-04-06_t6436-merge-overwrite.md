# t6436-merge-overwrite

Date: 2026-04-06

## Baseline

- `./scripts/run-tests.sh t6436-merge-overwrite.sh` baseline was **9/18**.
- Reproduced failing cases:
  - staged re-added / staged-then-removed path overwrite refusal (`#7`, `#8`)
  - untracked leading-path blockers (`#11`, `#12`)
  - unborn branch merge overwrite/state handling (`#14`, `#15`, `#16`, `#18`)

## Implemented fixes

### 1) Merge preflight now treats conflict-only paths as merge-touched

- File: `grit/src/commands/merge.rs`
- Function: `bail_if_merge_would_overwrite_local_changes`
- Change:
  - Added `merge_touched_paths` across all stages in the computed merge index.
  - In staged-change checks, when both HEAD and stage-0 target entries are absent for a path,
    we now still treat the path as changed if it appears in higher-stage conflict entries.
- Effect:
  - staged re-added paths are correctly protected from overwrite (`t6436` tests `#7`, `#8`).

### 2) Detect untracked leading-path blockers before checkout

- File: `grit/src/commands/merge.rs`
- Function: `bail_if_merge_would_overwrite_local_changes`
- Change:
  - For each incoming stage-0 path, preflight now checks parent components.
  - If a parent component exists in working tree as a non-directory file/symlink and is not tracked,
    it is reported as untracked overwrite conflict (e.g. `sub` for incoming `sub/f`).
- Effect:
  - Avoids late `create_dir_all` filesystem errors and emits Git-compatible merge refusal diagnostics.
  - Fixes untracked leading-path scenarios (`t6436` tests `#11`, `#12`).

### 3) Reworked unborn-branch merge flow to be preflight-first and index-preserving

- File: `grit/src/commands/merge.rs`
- Function: `merge_unborn`
- Added helper: `first_unborn_untracked_overwrite_path(...)`
- Changes:
  - Build target index first; run unborn untracked preflight before mutating HEAD.
  - On unborn untracked conflict, emit:
    - `error: Untracked working tree file '<path>' would be overwritten by merge.`
    - `fatal: read-tree failed`
    - exit 128.
  - Preserve existing current stage-0 index entries that do not conflict with target tree paths.
  - Only after successful preflight + merge of index entries, update HEAD and checkout.
  - Checkout now receives prior stage-0 map so unchanged preserved entries are not rewritten.
- Effect:
  - failed unborn merge keeps HEAD unborn and preserves index/worktree (`#14`, `#15`, `#16`).
  - successful unborn merge no longer clobbers existing staged/index content (`#18`).

## Validation

- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6436-merge-overwrite.sh` → **18/18**
- Harness:
  - `./scripts/run-tests.sh t6436-merge-overwrite.sh` → **18/18**
- Targeted regressions:
  - `./scripts/run-tests.sh t6439-merge-co-error-msgs.sh` → **6/6**
  - `./scripts/run-tests.sh t6426-merge-skip-unneeded-updates.sh` → **13/13**
  - `./scripts/run-tests.sh t6406-merge-attr.sh` → **13/13**
- Quality gates:
  - `cargo fmt`
  - `cargo clippy --fix --allow-dirty` (reverted unrelated clippy edits)
  - `cargo test -p grit-lib --lib` → **98/98**
