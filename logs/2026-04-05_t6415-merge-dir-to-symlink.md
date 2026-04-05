## t6415-merge-dir-to-symlink

Date: 2026-04-05

### Baseline
- Harness before fixes: `19/24`.
- Direct run failures:
  - `checkout does not clobber untracked symlink`
  - `a/b-2/c/d is kept when clobbering symlink b`
  - `do not lose untracked in merge (recursive)`
  - `do not lose modifications in merge (resolve)`
  - `do not lose modifications in merge (recursive)`
  - (`do not lose untracked in merge (resolve)` was still marked expected-failure in test file)

### Root causes
1. **`rm --cached` symlink path misclassified as directory**
   - In `rm`, recursion checks used `abs_path.is_dir()`, which follows symlinks.
   - For tracked symlink `a/b -> b-2`, `is_dir()` returned true because target is a directory.
   - That incorrectly triggered `not removing 'a/b' recursively without -r`.

2. **Merge lacked preflight protection against local-data overwrite in dir→symlink transitions**
   - Merge would proceed and cleanly replace `a/b/*` with symlink `a/b`, clobbering:
     - untracked files under that directory (e.g. `a/b/c/e`)
     - dirty tracked files (e.g. modified `a/b/c/d`)
   - Git is expected to refuse these merges.

### Fixes
1. **`grit/src/commands/rm.rs`**
   - Use `fs::symlink_metadata` in recursion checks to treat final-component symlinks as files, not directories.
   - Use metadata-driven removal dispatch in phase 2:
     - remove directories with `remove_dir_all`
     - remove symlinks/files with `remove_file`
   - Preserves existing directory behavior while fixing symlink edge case.

2. **`grit/src/commands/merge.rs`**
   - Added `bail_if_merge_would_overwrite_local_changes(...)` preflight in `do_real_merge` before writing index/worktree.
   - Detects and aborts when merge would overwrite:
     - dirty tracked files touched by merge
     - untracked files inside tracked directories being replaced by file/symlink paths
   - Added helper `is_worktree_entry_dirty(...)` for tracked file/symlink dirtiness checks.
   - Error messages now match expected merge refusal semantics.

3. **`tests/t6415-merge-dir-to-symlink.sh`**
   - Flipped:
     - `test_expect_failure 'do not lose untracked in merge (resolve)'`
       → `test_expect_success ...`
   - Allowed by project rule after underlying bug is fixed.

### Validation
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6415-merge-dir-to-symlink.sh`
  - Result: **24/24 pass**.
- Harness:
  - `./scripts/run-tests.sh t6415-merge-dir-to-symlink.sh`
  - Result: **24/24 pass**.

### Regression checks
- `./scripts/run-tests.sh t6421-merge-partial-clone.sh` → 3/3
- `./scripts/run-tests.sh t6400-merge-df.sh` → 7/7
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → 7/7
- `./scripts/run-tests.sh t3600-rm.sh` → 49/82 (snapshot only; suite still partial overall)
