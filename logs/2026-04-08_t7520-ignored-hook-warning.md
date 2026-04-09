# t7520-ignored-hook-warning

## Problem

Harness reported 3/5: tests 3–4 failed because `test_hook --disable` was a no-op in `tests/test-lib.sh` (unknown `--disable` broke out of the option loop early, so the hook stayed executable and no "hook was ignored" warning appeared).

## Fix

- Extended `test_hook` to match upstream `git/t/test-lib-functions.sh`: `--disable`, `--remove`, reject unknown `-` flags, resolve `$git_dir/hooks` via `git rev-parse --absolute-git-dir` (works for bare repos with `-C`).
- Aligned the second line of the ignored-hook hint with current Git: ``git config set advice.ignoredHook false``.

## Verification

- `./scripts/run-tests.sh t7520-ignored-hook-warning.sh` → 5/5
- `cargo test -p grit-lib --lib` → pass
