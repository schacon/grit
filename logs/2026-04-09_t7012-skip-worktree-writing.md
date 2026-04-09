# t7012-skip-worktree-writing

## Failures addressed

1. **Test 11 (stash + sparse + `git diff --quiet HEAD ":!modified"`)**
   - `parse_rev_and_paths` treated `:!modified` as a second revision → wrong diff mode and bogus errors.
   - `filter_by_paths` did not implement exclude pathspecs (`:!` / `:^`).
   - `diff_tree_to_worktree` still diffed `skip-worktree` paths vs tree (Git omits them for `git diff <rev>`).
   - After `stash push`, `addme` stayed as untracked on disk; `sparse-checkout set` must remove untracked paths outside the cone (Git behavior; `test_path_is_missing addme`).

## Changes

- `grit/src/commands/diff.rs`: path-aware rev/path split for `:!` / `:^`; filter with default include `.` when only exclusions; pass raw pathspecs into filter (resolved paths only for blob/two-path modes).
- `grit-lib/src/diff.rs`: skip `skip-worktree` in `diff_tree_to_worktree`; dedupe `diff_index_to_worktree` skip/assume-unchanged handling.
- `grit/src/commands/sparse_checkout.rs`: after writing index, walk worktree and delete untracked files/dirs not in sparse patterns (skip `.git`).

## Verification

- `cargo fmt`, `cargo clippy --fix --allow-dirty`, `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7012-skip-worktree-writing.sh` → 11/11
