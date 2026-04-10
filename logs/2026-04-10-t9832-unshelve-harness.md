# t9832-unshelve / git-p4 harness

## Problem

- `lib-git-p4.sh` required `PYTHON` prereq but `tests/test-lib.sh` never set it, so all git-p4 tests skipped as "python not available".
- Grit resolved `git-<cmd>` helpers only from `--exec-path` or the grit binary directory, ignoring `GIT_EXEC_PATH`, so vendored `git/git-p4.py` could not be used the way upstream Git does.

## Changes

- **`tests/test-lib.sh`**: Export `PYTHON_PATH` early; set `PYTHON` prereq when `NO_PYTHON` is unset; in `setup_trash`, set `GIT_EXEC_PATH` to a per-test dir containing a `git-p4` wrapper that runs `PYTHON_PATH GIT_SOURCE_DIR/git-p4.py`.
- **`grit/src/main.rs`**: Add `git_exec_path_for_helpers()` (GIT_EXEC_PATH, then CLI `--exec-path`, then binary dir); use it for `git-<cmd>` lookup in unknown-command help and for `git --exec-path` output.
- **`grit/src/alias.rs`**: Use `git_exec_path_for_helpers` in `try_exec_dashed`.

## Verification

- Manual: `GIT_EXEC_PATH` dir with stub `git-p4` — `grit p4` runs stub; `grit --exec-path` prints `GIT_EXEC_PATH` when set.
- `cargo check -p grit-rs`, `cargo test -p grit-lib --lib` pass.
- Full t9832 run in this image skips at `p4`/`p4d` missing (expected without Helix CLI); harness reports 0 tests when skip_all.
