# 2026-04-06 — t0614-reftable-fsck

## Scope
- Target file: `tests/t0614-reftable-fsck.sh`
- Start: `0/7` passing
- Goal: full pass by aligning reftable stack initialization and `refs verify` behavior with upstream

## Root causes
1. Reftable repositories created in test flows were not consistently initialized with a valid `reftable/tables.list` stack entry under all format-selection paths (notably default format via env/config used by tests).
2. `worktree add` did not initialize per-worktree reftable stacks, so stack verification for linked worktrees failed immediately.
3. `refs verify` did not validate reftable stacks/tables with Git-compatible error semantics, and did not fsck refs from reftable storage.
4. Reftable backend detection/opening paths were too brittle around missing config/`tables.list` in transitional states used by tests.
5. Symbolic-ref fsck diagnostics for invalid referents did not match expected `badReferentName` wording.

## Implemented fixes

### `grit/src/commands/init.rs`
- Updated ref-format selection precedence to align with upstream test expectations:
  1. `--ref-format`
  2. `GIT_DEFAULT_REF_FORMAT`
  3. `GIT_TEST_DEFAULT_REF_FORMAT`
  4. `init.defaultRefFormat`
  5. detected existing format on reinit (if not explicit)
  6. fallback `files`
- Preserved mismatch guard on reinit.
- Ensured resolved format is passed through directory creation so reftable layout is initialized when selected.

### `grit/src/commands/worktree.rs`
- Added reftable stack initialization for linked worktrees in `worktree add` flows:
  - creates `.../worktrees/<id>/reftable/`
  - writes initial table `00000000-00000000-00000000.ref` when absent
  - writes `tables.list` containing that table when absent

### `grit/src/commands/refs.rs`
- Implemented reftable-aware verification path in `refs verify`:
  - stack discovery for main repo and linked worktrees
  - `tables.list` parsing and table-file readability checks
  - reftable reader validation per table
  - table-name format validation (`badReftableTableName` warnings)
  - refname and symref target validation (`badRefName`, `badReferentName`) with `fsck.badRefName` level handling
  - object existence checks for direct refs
- Added Git-compatible fatal stack diagnostics:
  - `error: reftable stack is broken`
  - `error: reftable stack for worktree '<id>' is broken`
- Updated loose-ref symref invalid-target message to Git-style `badReferentName` phrasing.

### `grit-lib/src/reftable.rs`
- Hardened backend detection:
  - `is_reftable_repo()` now safely handles missing/unreadable local config and checks shared config via `commondir` for linked worktrees.

## Validation
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t0614-reftable-fsck.sh` ✅ `7/7` passing
- `./scripts/run-tests.sh t1407-worktree-ref-store.sh` ✅ `4/4` passing (regression check)
- `./scripts/run-tests.sh t1302-repo-version.sh` ✅ `18/18` passing (regression check)
- `cargo fmt && cargo clippy --fix --allow-dirty && cargo test -p grit-lib --lib` ✅
  - reverted unrelated clippy edits in non-target files

## Tracking updates
- `PLAN.md`: marked `t0614-reftable-fsck` complete (`7/7`) and refreshed global totals.
- `progress.md`: updated counts and added `t0614` entry under recently completed.
- `data/file-results.tsv`: refreshed `t0614` row to `7/7`.
- `test-results.md`: prepended test evidence for this increment.
