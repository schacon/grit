# t2015-checkout-unborn

## Issue

`check_dirty_worktree` removed untracked files that blocked checkout when the blob did not match the target tree. With unborn HEAD (no commits), that let `git checkout -b new origin` succeed while overwriting an untracked `file`, failing t2015 tests 2–4.

## Fix

In `check_dirty_worktree`, when `head_state.is_unborn()`, treat non-matching untracked paths as checkout conflicts instead of deleting the file and continuing (preserves t3501-style stale-file behavior for repos with a real HEAD).

## Validation

- `./scripts/run-tests.sh t2015-checkout-unborn.sh` — 6/6 pass
- `cargo test -p grit-lib --lib`
