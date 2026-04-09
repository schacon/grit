# t9510-ls-files-error

## Problem

All cases after the first failed because `tests/test-lib-tap.sh` runs each test body in the same shell without resetting `cwd`. The setup test ends inside `repo/`, so the next test’s `cd repo` tried `repo/repo` and exited 1 before running `grit`.

`tests/test-lib.sh` must not be modified (AGENTS.md), so the fix is confined to this test file.

## Fix

Prefix each test body with `cd "$TRASH_DIRECTORY"` (and combine with `cd repo` or `cd cross-repo` as needed) so every case starts from the trash root.

## Verification

`./scripts/run-tests.sh t9510-ls-files-error.sh` → 32/32 pass.
