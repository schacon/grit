# Cherry-pick conflict: no HEAD reflog entry

## Problem

`t8003-blame-corner-cases` test 15 failed because `git show HEAD@{1}:rodent` after a failed cherry-pick did not match Git: grit wrote a no-op reflog line (`old==new`) on conflict, shifting `HEAD@{1}` to the current tip instead of the previous checkout (the rodent commit).

## Fix

Removed `append_reflog` calls from the cherry-pick conflict path in `grit/src/commands/cherry_pick.rs`. Git does not append HEAD reflog when cherry-pick stops with conflicts and HEAD OID is unchanged.

## Validation

- `./scripts/run-tests.sh t8003-blame-corner-cases.sh` → 30/30
- `./scripts/run-tests.sh t13360-cherry-pick-allow-empty.sh` → 30/30
- `cargo test -p grit-lib --lib`
