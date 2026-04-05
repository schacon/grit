## 2026-04-05 — t1015-read-index-unmerged

### Goal
Make `t1015-read-index-unmerged.sh` fully pass.

### Initial status
- `./scripts/run-tests.sh t1015-read-index-unmerged.sh` => **4/6 passing**
- Failing tests:
  - `git merge --abort succeeds despite D/F conflict`
  - `git am --skip succeeds despite D/F conflict`

### Root causes
1. **Merge/am checkout helpers failed on D/F parent conflicts**
   - In both `merge` and `am`, checkout helpers created parent directories with
     `create_dir_all(parent)` without first removing a blocking file at that parent
     path.
   - In D/F conflict scenarios (e.g. `foo` file replaced by `foo/bar`), this caused
     `File exists`/`Not a directory` failures during abort/skip cleanup.

2. **`format-patch -1 <commit>` emitted the wrong patch**
   - `grit format-patch` treated `-1` as “last 1 commit from HEAD” instead of matching
     git behavior where `-1 <rev>` means “exactly that commit”.
   - This caused `t1015` to apply `initial` in one scenario instead of `directory and edit`,
     masking expected conflict behavior and making `am --skip` assertions fail.

### Implementation
- **`grit/src/commands/merge.rs`**
  - Added `prepare_parent_directory_for_path(abs_path: &Path) -> Result<()>`.
  - Before writing each entry in `checkout_entries`, call this helper:
    - remove non-directory parent component if present,
    - then create the parent directory tree.

- **`grit/src/commands/am.rs`**
  - Added analogous `prepare_parent_directory_for_path(abs_path: &Path) -> Result<()>`.
  - Updated `checkout_index_to_worktree` to call helper before file/symlink writes.

- **`grit/src/commands/format_patch.rs`**
  - Updated `run()` dispatch logic for revision handling:
    - when `revision` starts with `-<n>` and there is no explicit revision target:
      keep “last N commits” behavior,
    - when `revision` starts with `-<n>` and there *is* an explicit revision token,
      treat that explicit revision as the target commit/range and ignore the count for
      commit selection.
  - This restores expected git-like semantics for calls such as:
    - `git format-patch -1 d-edit` → patch for `d-edit`.

### Validation
- `cargo fmt && cargo clippy --fix --allow-dirty && cargo test -p grit-lib --lib` — success.
  - Reverted unrelated clippy edits in non-target files.
- `cargo build --release -p grit-rs` — success.
- `GUST_BIN=/workspace/tests/grit bash tests/t1015-read-index-unmerged.sh` — **6/6 passing**.
- `./scripts/run-tests.sh t1015-read-index-unmerged.sh` — **6/6 passing**.

### Files changed
- `grit/src/commands/merge.rs`
- `grit/src/commands/am.rs`
- `grit/src/commands/format_patch.rs`
- `data/file-results.tsv`
- `PLAN.md`
- `progress.md`
- `test-results.md`
## t1015-read-index-unmerged

- Read `/Users/schacon/projects/grit/AGENTS.md`, the `t1015-read-index-unmerged` entry in `PLAN.md`, and upstream `git/t/t1015-read-index-unmerged.sh`.
- Ran the requested command:
  `CARGO_TARGET_DIR=/tmp/grit-build-t1015 bash scripts/run-upstream-tests.sh t1015 2>&1 | tail -40`
  which initially reported `4/6` passing against `/Users/schacon/projects/grit/target/release/grit` once the release binary was rebuilt.
- Unblocked the build first by resolving stray merge-conflict markers in `/Users/schacon/projects/grit/grit/src/main.rs`, which were preventing `cargo build --release`.
- Reproduced the two failing cases directly and confirmed:
  `git merge d-edit` aborted before writing `MERGE_HEAD` on a D/F conflict, and `git format-patch -1 d-edit` selected the wrong commit, causing `git am -3` to apply cleanly instead of entering a skip-able session.
- Updated `/Users/schacon/projects/grit/grit/src/commands/merge.rs` to prepare worktree paths safely during conflict materialization and abort cleanup, removing blocking file/directory ancestors so D/F conflicts can be recorded and later aborted.
- Updated `/Users/schacon/projects/grit/grit/src/commands/am.rs` to detect D/F conflicts during patch application and 3-way fallback, preserve the `am` session on conflict, and cleanly restore worktree/index paths during `am --skip` and abort-like resets.
- Updated `/Users/schacon/projects/grit/grit/src/commands/format_patch.rs` so `format-patch -1 <rev>` emits the named commit itself instead of treating `<rev>` as a lower bound.
- Rebuilt with `cargo build --release` and confirmed the requested upstream harness command now reports `6/6` passing for `t1015-read-index-unmerged`.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t1015 cargo fmt` successfully.
- Attempted `CARGO_TARGET_DIR=/tmp/grit-build-t1015 cargo clippy --fix --allow-dirty`, but the sandbox blocked Cargo's TCP-based lock manager setup with `Operation not permitted (os error 1)`.
- Updated `PLAN.md` and `progress.md` to mark `t1015-read-index-unmerged` complete.
