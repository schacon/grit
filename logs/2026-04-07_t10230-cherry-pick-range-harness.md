# t10230-cherry-pick-range

## Issue

`t10230-cherry-pick-range.sh` reported 4/31 then 28/31 passes. Root causes:

1. **`test_expect_success` cwd**: Each test body ran in the parent shell. A block ending inside `repo/` left the next test’s `cd repo` resolving to `repo/repo`. Fixed by starting each success test from `TRASH_DIRECTORY` inside `_test_eval_inner` and `cd "$TRASH_DIRECTORY"` after the body so the parent shell returns to trash root.

2. **`test_must_fail grit`**: `test_must_fail_acceptable` only allowed `git`, so `test_must_fail grit cherry-pick a1` failed before running grit, skipping conflict setup. Added `grit` to the allowed command list.

## Verification

`./scripts/run-tests.sh t10230-cherry-pick-range.sh` → 31/31.
