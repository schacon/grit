# t4039-diff-assume-unchanged

## Failure

`git diff-index HEAD^ -- one` grepped for the index blob of `one` after `update-index --assume-unchanged` and worktree corruption. Grit reported `A` with all-zero new oid in raw output and/or refreshed from the work tree.

## Fix (`grit/src/commands/diff_index.rs`)

1. **`diff_tree_vs_worktree`**: Skip refreshing the tree↔index “new” side from the work tree and skip index↔worktree follow-up when the index entry has `assume_unchanged` (same idea as `skip_worktree` / Git `CE_VALID`).
2. **Raw `diff-index` output**: For uncached `Added` lines, Git still prints all-zero placeholders in the usual case (t1501). When the path has assume-unchanged set, print the real index blob oid on the new side so scripts can match the staged content without reading the work tree (t4039).

## Validation

- `./scripts/run-tests.sh t4039-diff-assume-unchanged.sh` — 4/4
- `cargo test -p grit-lib --lib`
