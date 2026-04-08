# t9760-hash-object-blob-tree

## Issue

Harness reported 1/32 passing: first test left cwd inside `repo/`, so later `cd repo` tried `repo/repo` and failed.

## Fix

1. Setup: use `git -C repo config` instead of `cd repo` so trash cwd stays at trash root.
2. All tests: `cd "$TRASH_DIRECTORY/repo"` so each block enters the nested repo from an absolute path regardless of prior test cwd.

## Verification

`./scripts/run-tests.sh t9760-hash-object-blob-tree.sh` → 32/32.
