# t6011-rev-list-with-hierarchies

## Problem

`t6011-rev-list-with-hierarchies.sh` failed at test 1: after `git checkout -b topic B`, `git checkout main` could not find `main`.

Root cause: `tests/test-lib.sh` `setup_trash` runs `grit init` before each test file without `-b`. Grit defaulted `HEAD` to `refs/heads/master`. The test then runs `git init -b main`, which **reinitializes** an existing repo; Git ignores `-b` when HEAD already exists, so the branch stayed `master`. The merge and all following rev-list checks failed or cascaded.

## Fix

Changed `grit init` fallback initial branch from `master` to `main` when neither `-b`/`--initial-branch`, `GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME`, nor `init.defaultBranch` applies. Matches current Git default and aligns trash repos with tests that assume `main`.

## Verification

- `./scripts/run-tests.sh t6011-rev-list-with-hierarchies.sh` → 28/28 pass
- `cargo test -p grit-lib --lib` → pass
