## Task
- Target: `t4040-whitespace-status.sh`
- Status: claimed in progress.

## Initial notes
- Goal is to close remaining 4 failing tests around `git diff --check` / `--exit-code` whitespace handling and exit statuses.
- Next steps:
  1. run baseline test file and capture failing cases;
  2. map each failure to `grit diff`/`grit diff-index` whitespace-check behavior;
  3. implement minimal fixes and rerun target + regressions.

## Investigation summary
- Baseline local run:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4040-whitespace-status.sh`
  - Result: 7/11 passing, failures in tests 3/5/7/9.
- Root causes identified:
  1. `diff-tree` did not accept `-b`/`--ignore-space-change`.
  2. `diff-index` accepted `-b` after parser update, but did not filter whitespace-only modifications before exit-code evaluation.
  3. `diff-files` accepted whitespace ignore flags in parser but treated them as no-ops.
- Implemented command-level parity:
  - `grit/src/commands/diff_tree.rs`
    - parse `-b` / `--ignore-space-change`
    - apply post-diff filtering that normalizes whitespace runs and drops whitespace-only modified entries
  - `grit/src/commands/diff_index.rs`
    - parse `-b` / `--ignore-space-change`
    - apply whitespace-only modified-entry filtering prior to output and `--exit-code` check
  - `grit/src/commands/diff_files.rs`
    - parse `-b` / `-w` / `--ignore-space-at-eol` / `--ignore-blank-lines`
    - apply normalized content filtering on generated `DiffEntry` values so whitespace-only entries are removed before output/exit-code.

## Validation
- `cargo build --release` ✅
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4040-whitespace-status.sh` ✅ 11/11
- `./scripts/run-tests.sh t4040-whitespace-status.sh` ✅ 11/11
- `bash scripts/run-upstream-tests.sh t4040-whitespace-status` ✅ 11/11
- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅ (unrelated autofixes reverted)
- `cargo test -p grit-lib --lib` ✅
