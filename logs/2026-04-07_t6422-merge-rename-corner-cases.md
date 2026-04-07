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

### Next concrete debugging target
- Start with failing block #7/#8/#9 (rename-directory trio) to align:
  - conflict path naming,
  - stage population,
  - worktree file placement for D/F rename collisions.
