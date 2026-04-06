## t1020-subdirectory

### Goal
Make `tests/t1020-subdirectory.sh` fully pass.

### Initial state
- `./scripts/run-tests.sh t1020-subdirectory.sh` reported **11/15**.
- Failing cases:
  - `FAIL 4: diff-files`
  - `FAIL 9: !alias expansion`
  - `FAIL 10: GIT_PREFIX for !alias`
  - `FAIL 11: GIT_PREFIX for built-ins`

### Root causes
1. `diff-files --name-only .` from a subdirectory did not scope `.` relative to CWD.
2. Shell aliases (`!alias`) ran from the current subdirectory instead of repository root.
3. `GIT_PREFIX` was not exported for subdirectory invocations.
4. `git diff` did not invoke external diff programs (`GIT_EXTERNAL_DIFF` / `diff.external`) in these paths.

### Code changes
- `grit/src/commands/diff_files.rs`
  - Normalized pathspecs relative to worktree/CWD before matching.
  - Reused shared pathspec matching helper (`crate::pathspec::pathspec_matches`).
  - Added handling so `.` maps to current subdirectory prefix.
- `grit/src/main.rs`
  - Added `refresh_git_prefix_env()` called after global option processing.
  - Computes and exports `GIT_PREFIX` (or clears it outside a worktree).
  - Updated `run_alias()` for `!alias` to execute from repository root.
- `grit/src/commands/diff.rs`
  - Added external diff command resolution:
    - `GIT_EXTERNAL_DIFF` env var
    - `diff.external` config fallback
    - disabled by `--no-ext-diff`
  - Added external diff invocation path with Git-compatible argument order and
    `GIT_DIFF_PATH_COUNTER` / `GIT_DIFF_PATH_TOTAL`.

### Validation
- `cargo fmt`
- `cargo build --release -p grit-rs`
- `rm -rf /workspace/tests/trash.t1020-subdirectory && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1020-subdirectory.sh` → **15/15 pass**
- `./scripts/run-tests.sh t1020-subdirectory.sh` → **15/15 pass**
- `cargo clippy --fix --allow-dirty` (reverted unrelated edits)
- `cargo test -p grit-lib --lib` → **96 passed**
