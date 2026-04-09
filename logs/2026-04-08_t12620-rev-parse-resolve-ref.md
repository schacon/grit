# t12620-rev-parse-resolve-ref

## Issue

All cases after `setup` failed with `cd: repo: No such file or directory` because the setup block ended with `cd repo` in the main shell, so the trash cwd was left inside `repo/`. Later tests use `(cd repo && …)` relative to the trash root, so `repo` was not found.

## Fix

Wrapped the body of the setup `test_expect_success` in a subshell `( cd repo && … )` so the outer shell stays at the trash directory after setup completes.

## Verification

- `./scripts/run-tests.sh t12620-rev-parse-resolve-ref.sh` — 32/32 pass
- `./scripts/run-tests.sh t6409-merge-subtree.sh` — 12/12 pass (confirmed no regression from avoiding a global `cd "$TRASH_DIRECTORY"` between tests)
