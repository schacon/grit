# t0034-root-safe-directory

## Problem

- Discovery only enforced `safe.directory` when `GIT_TEST_ASSUME_DIFFERENT_OWNER` was set, so real root-owned repos were not rejected like Git.
- `clone` opened source repos without `safe.directory` / ownership checks (t0033 clone tests).
- Harness skipped t0034: missing `NOT_ROOT` lazy prereq, broken `SUDO` check (`sudo command` vs shell builtin, PATH for grit wrapper), empty `TEST_SHELL_PATH` in `run_with_sudo`.

## Changes

- `grit-lib`: Unix `ensure_valid_ownership` (root + `SUDO_UID` like `git-compat-util.h`), run on discovery when not using test hook; refactored `safe.directory` matching; `verify_safe_for_clone_source` for clone.
- `grit clone`: call verify after opening source.
- `tests/t0034-root-safe-directory.sh`: `NOT_ROOT` prereq, fix `SUDO` lazy prereq.
- `tests/lib-sudo.sh`: default shell + `PATH` for sudo.
- `scripts/run-tests.sh`: set `GIT_TEST_ALLOW_SUDO=YES` for t0034 only.

## Verification

- `./scripts/run-tests.sh t0033-safe-directory.sh` (with env) — 22/22
- `./scripts/run-tests.sh t0034-root-safe-directory.sh` — 8/8
- `cargo test -p grit-lib --lib`
