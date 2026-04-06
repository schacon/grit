## 2026-04-06 — t6102-rev-list-unexpected-objects

### Scope
- Claimed `t6102-rev-list-unexpected-objects` as the next highest-priority remaining `t6*` item.

### Initial actions
- Marked `t6102-rev-list-unexpected-objects` as in progress (`[~]`) in `PLAN.md`.
- Updated `progress.md` counts to keep completed/in-progress/remaining aligned with the plan.
- Next: reproduce current failures directly and via harness, then implement missing `rev-list` unexpected-object handling semantics.

### Reproduction summary
- Direct run (`GUST_BIN=/workspace/target/release/grit bash tests/t6102-rev-list-unexpected-objects.sh`) initially failed at 15/22, then 19/22 after first code update.
- Failures were concentrated in object-type handling paths for `rev-list --objects`:
  - lone/seen malformed tree entry type checks (`not a blob`, `not a tree`)
  - tagged-object expected type validation (`not a commit`, `not a tree`, `not a blob`).

### Root cause
- `rev-list` resolution always peeled positive specs to commits before object collection, so non-commit tips used by this suite were rejected or mishandled.
- Tree traversal accepted mismatched entry object kinds too leniently (returned early), instead of raising corruption errors matching expected diagnostics.
- Tagged roots needed explicit expected-type validation (from tag header `type`) when traversed in `--objects` mode.

### Implemented fixes
- Updated `grit-lib/src/rev_list.rs`:
  - Added object-root-aware positive spec handling for `--objects`:
    - commit-resolvable specs still seed commit walk,
    - non-commit roots are collected and traversed as object roots.
  - Added root-object expected-type tracking and validation:
    - for annotated tags, enforce declared tag target type (`commit`/`tree`/`blob`) and error with `object <input> is not a <type>` when mismatched.
  - Hardened tree traversal checks:
    - tree entries with mode `040000` must point to tree objects (`... is not a tree`),
    - non-tree entries must point to blob objects (`... is not a blob`).
  - Kept missing-object behavior consistent with existing `MissingAction` handling.

### Validation
- `PATH="/tmp:$PATH" GUST_BIN="/workspace/target/release/grit" bash tests/t6102-rev-list-unexpected-objects.sh` → **22/22 pass** (using temporary `hex2oct` helper in `/tmp` due simplified local `tests/test-lib.sh` not sourcing upstream helper functions).
- `PATH="/tmp:$PATH" ./scripts/run-tests.sh t6102-rev-list-unexpected-objects.sh` → **22/22 pass**.
- Regression checks:
  - `./scripts/run-tests.sh t6004-rev-list-path-optim.sh` → 7/7 pass.
  - `./scripts/run-tests.sh t6005-rev-list-count.sh` → 6/6 pass.
  - `./scripts/run-tests.sh t6131-pathspec-icase.sh` → 9/9 pass.

### Status
- Marked `t6102-rev-list-unexpected-objects` as done (`[x]`) in `PLAN.md`.
