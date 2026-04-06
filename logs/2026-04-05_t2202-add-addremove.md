## t2202-add-addremove — 2026-04-05

- Claimed next small checkout/add task after `t2027`: `t2202-add-addremove` (0/3).
- Reproduced failures with:
  - `./scripts/run-tests.sh t2202-add-addremove.sh` → 0/3
  - `GUST_BIN=/workspace/tests/grit TEST_VERBOSE=1 bash tests/t2202-add-addremove.sh`
- Root cause:
  - Global option parser rejected `--literal-pathspecs` before command dispatch, causing setup to fail before add logic executed.
  - Follow-on test failures were cascading due to missing initial commit/HEAD.

- Implementation:
  - Updated global option extraction in `grit/src/main.rs` to accept:
    - `--literal-pathspecs`
    - `--glob-pathspecs`
    - `--noglob-pathspecs`
    - `--icase-pathspecs`
  - Mapped these global flags to environment variables in `apply_globals()`:
    - `GIT_LITERAL_PATHSPECS=1`
    - `GIT_GLOB_PATHSPECS=1`
    - `GIT_NOGLOB_PATHSPECS=1`
    - `GIT_ICASE_PATHSPECS=1`
  - This matches Git’s global pathspec option handling style and unblocks `git --literal-pathspecs add --all`.

- Validation:
  - `cargo fmt` ✅
  - `cargo build --release -p grit-rs` ✅
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t2202-add-addremove.sh` ✅ (3/3)
  - `./scripts/run-tests.sh t2202-add-addremove.sh` ✅ (3/3)
  - `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated churn files afterward)
  - `cargo test -p grit-lib --lib` ✅ (96/96)
