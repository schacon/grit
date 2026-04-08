# t6424-merge-unrelated-index-changes

## Summary

Fixed `grit merge` to match Git’s index/worktree safety rules exercised by t6424:

- **Fast-forward**: compose index from target tree plus staged paths that are true additions (not in `HEAD^{tree}`); do not carry removed HEAD paths that are absent from the target.
- **Overwrite checks**: treat staged conflicts only when the merge result *touches* a path vs HEAD; handle staged removals; skip “staged matches target” shortcut so unrelated additions still conflict when appropriate.
- **Index cleanliness**: `index_matches_head_tree` (entry-by-entry vs HEAD tree) for octopus, recursive/ort/subtree, ours/theirs; resolve strategy uses stricter per-path check.
- **Multiple `-s`**: `Args.strategy` is `Vec` with `ArgAction::Append`; `try_merge_strategies` tries each, restores pre-merge index on total failure; `do_real_merge` can bail on conflict without `exit` when probing strategies.
- **Octopus**: pre-simulate for conflicts + overwrite check; restore pre-merge index on failure; merge unrelated staged paths into final index when merge succeeds.
- **Harness**: `test_path_exists` in `test-lib-harness.sh` (t6424 uses it).

## Verification

- `./scripts/run-tests.sh t6424-merge-unrelated-index-changes.sh` → 19/19
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`
