# t3416-rebase-onto-threedots

## Summary

Made `tests/t3416-rebase-onto-threedots.sh` pass 18/18 by aligning `grit rebase` with Git’s `builtin/rebase.c` behavior for symmetric-diff `--onto`, `--keep-base`, and cherry-pick filtering.

## Changes (`grit/src/commands/rebase.rs`)

- Reject `--keep-base` together with `--onto` (Git message: options cannot be used together).
- When `--onto` contains `...`, resolve via merge bases of the two tips; if not exactly one base, error with `'<spec>': need exactly one merge base` (or `... with branch` for keep-base synthesized onto).
- `--keep-base`: set onto to merge base of `upstream...<branch>` (branch short name or `HEAD`), same error wording as Git when ambiguous.
- When `--keep-base` and effective `reapply_cherry_picks` (default true), use `onto` as the upstream for `collect_rebase_todo_commits` so cherry equivalence uses the same symmetric range as Git.
- `filter_cherry_equivalents` for interactive rebase when `--keep-base` and `--no-reapply-cherry-picks` so the todo list drops patch-id duplicates (test 17).

## Verification

- `cargo build --release -p grit-rs`
- Manual harness: `bash tests/t3416-rebase-onto-threedots.sh` (18 pass)
- `cargo test -p grit-lib --lib`
