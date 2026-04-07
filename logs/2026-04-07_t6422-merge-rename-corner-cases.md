## 2026-04-07 — t6422 baseline + investigation

### Claim / status
- Claimed `t6422-merge-rename-corner-cases` as active and set plan status to in-progress.

### Baseline runs
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6422-merge-rename-corner-cases.sh`
  - Result: **14/26 passing** (12 failing, 6 expected-failure TODO tests still marked TODO).
- Harness:
  - `./scripts/run-tests.sh t6422-merge-rename-corner-cases.sh`
  - Result: **14/26 passing**, TSV updated.

### Current failing subtests (direct)
- 7 `rename/directory conflict + clean content merge`
- 8 `rename/directory conflict + content merge conflict`
- 9 `disappearing dir in rename/directory conflict handled`
- 12 `handle rename/rename (2to1) conflict correctly`
- 13 `merge has correct working tree contents`
- 16 `rename/rename/add-dest merge still knows about conflicting file versions`
- 18 `rrdd-check: rename/rename(2to1)/delete/delete conflict`
- 19 `mod6-check: chains of rename/rename(1to2) and rename/rename(2to1)`
- 20 `check simple rename/rename conflict`
- 24 `check nested conflicts from rename/rename(2to1)`
- 25 `rename/rename(1to2) with a binary file`
- 26 `submodule/directory preliminary conflict`

### Key observations
- Suite has improved from prior plan snapshot (9/26) to 14/26.
- A harness compatibility issue in `tests/test-lib.sh` surfaced in this suite:
  - local `test_seq` lacked `-f <format>` support used in t6422;
  - added support locally, which removed `integer expression expected` noise and corrected fixture generation.
- Remaining failures are concentrated in merge-rename conflict modeling:
  - directory/file rename collision placement (`newfile~HEAD`/`sub~HEAD` behavior),
  - rename/rename stage shape and path mapping,
  - rename/add/delete multi-conflict classification,
  - submodule preliminary conflict stage shape.

### Code attempt summary
- Explored targeted edits in `grit/src/commands/merge.rs` around rename handling and stage deduplication.
- Attempted patch did not improve pass count and was reverted to keep the branch coherent.
- No functional merge behavior change has been committed yet for t6422 in this iteration.

## 2026-04-07 — Increment: 14/26 → 16/26

### Implemented fixes (committed increment)
- Updated `grit/src/commands/merge.rs` to handle rename/delete + rename/add overlap at the same rename target:
  - when ours renamed `base_path -> ours_new_path`, theirs deleted `base_path`, and theirs independently added at `ours_new_path`,
    we now classify as `rename/add` conflict at the destination (stage 2 ours, stage 3 theirs), instead of only `rename/delete`.
  - this resolves the `rad` scenario shape and unblocks subtest #17.
- Improved D/F rename-directory conflict handling:
  - detect when rename target collides with paths that only exist due to that same source rename (`base_path` nested under `ours_new_path`) and avoid treating that as directory/file collision.
  - for real directory/file collisions, stage and materialize conflict content at side-path (`<path>~HEAD`) so worktree/index shape matches expected `newfile~HEAD` behavior.
- `remove_deleted_files(...)` now only removes regular files and skips directories, avoiding unintended removal attempts on conflict-created directories.
- Conflict marker labels for rename content merges now preserve expected path-qualified labels:
  - ours label: `HEAD:<rename-target>`
  - theirs label: `<other>:<source-path>`

### Validation after this increment
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6422-merge-rename-corner-cases.sh`
  - **16/26 passing**.
- Harness:
  - `./scripts/run-tests.sh t6422-merge-rename-corner-cases.sh`
  - **16/26 passing**.

### Newly passing subtests
- #8 `rename/directory conflict + content merge conflict`
- #17 `rad-check: rename/add/delete conflict`

### Remaining failures (10)
- 7, 12, 13, 16, 18, 19, 20, 24, 25, 26

### Targeted regressions
- `./scripts/run-tests.sh t6400-merge-df.sh` → 7/7
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → 7/7
- `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh` → 2/2

### Next concrete debugging target
- Start with failing block #7/#8/#9 (rename-directory trio) to align:
  - conflict path naming,
  - stage population,
  - worktree file placement for D/F rename collisions.

### 2026-04-07 incremental progress (latest)

- Pushed `58c146d1`: improved rename/add conflict handling at rename target (rad case), moving suite from 14/26 → 15/26.
- Pushed `2c18087e`: improved rename-directory sidepath handling and conflict labels; suite moved to 16/26.
- Pushed `56533ee5`: improved rename conflict classification and stage replacement behavior; suite moved to 20/26.

#### New improvement in this turn
- Added additional rename/rename(2to1) handling in `grit/src/commands/merge.rs` for case-2 rename flow:
  - when case-1 already handled one source of a same-destination rename pair, preserve the other source path for case-2 processing instead of skipping it as fully handled.
  - this allows expected `rename/rename` conflict shaping for delete/delete-style paired renames.
- Validation:
  - direct: `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6422-merge-rename-corner-cases.sh` → **22/26**.
  - harness: `./scripts/run-tests.sh t6422-merge-rename-corner-cases.sh` → **22/26**.
  - targeted regressions:
    - `./scripts/run-tests.sh t6400-merge-df.sh` → 7/7
    - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → 7/7
    - `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh` → 2/2

#### Current remaining failing subtests
- 7 `rename/directory conflict + clean content merge`
- 19 `mod6-check: chains of rename/rename(1to2) and rename/rename(2to1)`
- 24 `check nested conflicts from rename/rename(2to1)`
- 26 `submodule/directory preliminary conflict`

## 2026-04-07 — t6422 progress to 20/26

### Summary
- Implemented additional merge conflict handling in `grit/src/commands/merge.rs` and improved t6422 from **14/26 → 20/26**.
- Current failing subtests:
  - #7 `rename/directory conflict + clean content merge`
  - #16 `rename/rename/add-dest merge still knows about conflicting file versions`
  - #18 `rrdd-check: rename/rename(2to1)/delete/delete conflict`
  - #19 `mod6-check: chains of rename/rename(1to2) and rename/rename(2to1)`
  - #24 `check nested conflicts from rename/rename(2to1)`
  - #26 `submodule/directory preliminary conflict`

### Implemented changes
- Added D/F-aware cleanup:
  - `remove_deleted_files(...)` now skips removing files that became `~HEAD` conflict sidepaths in merge output.
  - Prevents sidepath files from being deleted before conflict materialization.
- Improved conflict marker labels when writing staged conflict files to sidepaths:
  - threaded optional custom conflict labels through `try_content_merge(...)`.
  - for sidepath conflicts (`newfile~HEAD` style), use labels from original logical path:
    - ours: `HEAD:<logical-path>`
    - theirs: `<other>:<base-path>`
  - this fixed `rename-directory conflict + content merge conflict` expectations.
- Hardened stage writing helper:
  - `stage_entry(...)` now removes any existing entry at the same `(path, stage)` before pushing, avoiding duplicate stage records during complex rename interactions.
- Fixed rename/rename(2to1) misclassification:
  - in the “ours renamed + theirs deleted source” path, do not treat destination presence as rename/add when the other side’s destination came from a rename of a *different* source.
  - this corrected conflict type and stage shape for multiple rename/rename(2to1) scenarios.

### Validation
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6422-merge-rename-corner-cases.sh`
  - Result: **20/26 pass**.
- Harness:
  - `./scripts/run-tests.sh t6422-merge-rename-corner-cases.sh`
  - Result: **20/26 pass**; TSV updated.
- Targeted regressions:
  - `./scripts/run-tests.sh t6400-merge-df.sh` → 7/7
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → 7/7
  - `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh` → 2/2

### Notes
- Remaining failures are now clustered around:
  - untracked `out` behavior in #7 under local harness expectations,
  - deeper rename/rename + add-dest chain interactions (#16/#18/#19/#24),
  - submodule/directory preliminary conflict modeling (#26).
