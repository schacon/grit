# t3501-revert-cherry-pick

## Summary

Made `tests/t3501-revert-cherry-pick.sh` pass 21/21.

## Changes

- **grit-lib `merge_trees`**: Rename-aware three-way tree merge (50% threshold) for cherry-pick and revert; disambiguate multiple renames to the same path by similarity score.
- **grit-lib `commit_pretty`**: `--pretty=reference` one-liner for `show`/`log` and revert message bodies.
- **cherry-pick**: Use merge_trees; `cherry-pick -` resolves prior branch via HEAD reflog `checkout: moving from X to Y`; conflict reflog; preserve dirty worktree when index OID unchanged after merge; hidden `--reference` for upstream usage line.
- **revert**: merge_trees; root commits use empty parent tree; dirty-index check; reference / Reapply titles; conflict hints + `advice.mergeConflict`; `revert.reference` config; editor path for `--edit`; append_reflog.
- **checkout**: Reflog identity uses `GIT_*_DATE`; remove or skip blocking untracked files when switching (orphan/unborn leftovers); newline-tolerant blob match where applicable.
- **tests `test_commit`**: Implement `--append` (was ignored) so `double-add dream` test creates a real conflict.

## Validation

- `./scripts/run-tests.sh t3501-revert-cherry-pick.sh` → 21/21
- `cargo test -p grit-lib --lib`
