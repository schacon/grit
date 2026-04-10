# t3426-rebase-submodule

## Summary

Made `tests/t3426-rebase-submodule.sh` pass (29/29).

## Changes

1. **revert**: After `checkout_merged_index`, refresh index stat cache and rewrite index so `diff-files` / `diff-index` do not report spurious `M` with zero new OID (post-revert prelude in t3426).
2. **checkout** `check_dirty_worktree`: Do not auto-remove untracked files when the target is a gitlink; refuse like Git (t3426 untracked `sub1` file blocking submodule add).
3. **rebase** dirty check: Honor `diff.ignoreSubmodules=all` for staged/unstaged diffs; filter unstaged gitlink entries per `submodule.<name>.ignore` (`dirty`/`all`) from `.gitmodules` + config (t3426 interactive + modified submodule).
4. **submodule**: Expose `parse_gitmodules_with_repo` as `pub(crate)` for rebase.

## Validation

- `./scripts/run-tests.sh t3426-rebase-submodule.sh` — 29/29
- `cargo test -p grit-lib --lib`
