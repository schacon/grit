# t3429-rebase-edit-todo

## Goal

Make `tests/t3429-rebase-edit-todo.sh` pass (7/7).

## Changes (summary)

- **Canonical todo file:** write/read `.git/rebase-merge/git-rebase-todo` (with `GIT_REBASE_TODO` override), keep legacy `rebase-merge/todo` hex list for conflict `--continue`.
- **Replay loop:** process `pick`/`reword`/`squash`/`fixup`/`exec`; after each step, shrink the todo from disk (so exec-appended lines are visible); re-read todo after successful exec before popping.
- **`-x` / empty list:** inject `exec` lines into the script; do not use preemptive “up to date” when commits exist to replay; `rebase HEAD -x` still runs.
- **`--root` without `--onto`:** match Git’s synthetic squash-onto empty-root commit.
- **Interactive:** sequence editor produces full script; defer squash message editor when a fixup follows in the same chain; run editor after fixup via `pending-squash-msg` + `defer-squash-editor`.
- **`rebase --edit-todo`:** run sequence editor on the todo path.
- **`pull`:** extend `rebase::Args` struct init for new flags.

## Validation

- `./scripts/run-tests.sh t3429-rebase-edit-todo.sh` — 7/7
- `cargo test -p grit-lib --lib` — pass

## Note

Workspace `cargo clippy -p grit-rs -- -D warnings` currently fails on pre-existing `grit-lib` doc-comment lints unrelated to this change.
