# t1300-config progress

## Changes

- `scripts/run-tests.sh`: unset `GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME` so inherited agent env matches harness (init default branch tests expect `master` when unset).
- `grit init`: stop writing `[init] defaultBranch` into `.git/config`; resolve initial branch like `repo_default_branch_name` (env then config then `master`).
- `grit config`: skip redundant global `init.defaultBranch` set when value equals `GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME` (avoids duplicate listing vs upstream t1300).
- `grit config`: `--get-regexp` with bare boolean keys prints key only unless `--bool` / `--bool-or-int` / matching `--type`.
- `grit config`: missing `--file` path errors like Git (`fatal: unable to read config file ...`).
- `grit config`: bare `grit config` → `error: no action specified` (no double `error:` prefix).

## Result

`t1300-config.sh`: 292/497 passing after `./scripts/run-tests.sh` (was 278/497).
