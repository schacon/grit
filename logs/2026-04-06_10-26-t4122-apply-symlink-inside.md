## Task: t4122-apply-symlink-inside

### Claim
- Claimed after completing `t4010-diff-pathspec`.
- Marked as `[~]` in `PLAN.md`.

### Baseline
- Tracked harness:
  - `./scripts/run-tests.sh t4122-apply-symlink-inside.sh`
  - result: **1/7 passing** (6 failing)
- Direct local run from `tests/`:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4122-apply-symlink-inside.sh`
  - result: **1/7 passing** (same 6 failing tests)

### Current failing assertions
- baseline (before fixes): 1, 2, 3, 4, 6, 7 failed
- final result: all 7/7 pass

### Initial observations
- Root causes identified and fixed:
  1. `format-patch --binary -1 --stdout` parsed options/revision correctly, but generated patch text included the `-- ` signature separator as part of the last hunk body. `apply` consumed that as a context/remove line and failed hunk matching.
  2. `apply --index` compared worktree symlink paths using file-byte reads only; for symlink index entries this mismatched and rejected valid preimages.
  3. `apply` symlink safety checks only validated full target path and missed intermediate symlink components in some ordering scenarios.
  4. New-file precheck did not account for existing descendants under a soon-to-be-created symlink path (e.g. `foo` then `foo/bar` in one patch stream).

### Implemented fixes
- `grit/src/commands/format_patch.rs`
  - Added explicit `binary` flag parsing (`-B/--binary`) so command-line intent is recognized.
  - Added patch footer separator `"-- \n"` to generated output for compatibility.
- `grit/src/commands/apply.rs`
  - `parse_hunk` now stops parsing hunk bodies at patch separators/signatures (`"-- "` and `"--"`) in addition to next diff/hunk headers.
  - Added symlink-aware worktree/index comparison helper for `--index` mode: symlink entries hash link targets (`read_link`) instead of file bytes.
  - Added `ensure_no_symlink_prefix_conflict` and called it from worktree precheck and index apply paths.
  - Added descendant existence check for new-file paths (`path_has_descendant`) to prevent creating paths that conflict with pre-existing deeper paths under symlink-sensitive prefixes.

### Validation
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4122-apply-symlink-inside.sh` from `tests/`: **7/7**
- `./scripts/run-tests.sh t4122-apply-symlink-inside.sh`: **7/7**
- `bash scripts/run-upstream-tests.sh t4122-apply-symlink-inside`: **7/7**
- regressions:
  - `./scripts/run-tests.sh t4010-diff-pathspec.sh`: **17/17**
  - `./scripts/run-tests.sh t4153-am-resume-override-opts.sh`: **6/6**
