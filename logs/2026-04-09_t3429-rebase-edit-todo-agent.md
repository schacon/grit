# t3429-rebase-edit-todo (agent)

## Summary

Made `tests/t3429-rebase-edit-todo.sh` pass 7/7.

## Changes (grit)

- **Canonical todo file:** read/write `.git/rebase-merge/git-rebase-todo` (and legacy `todo`), honor `GIT_REBASE_TODO` with work-tree-relative resolution.
- **Re-read todo after steps:** outer loop in `replay_remaining`; trim todo on disk *before* global `-x` exec so appended lines survive.
- **Non-interactive `-x` + appended `exec` lines:** `parse_rebase_replay_step` parses interactive `exec`/`edit`/`merge` even when `interactive` marker is absent.
- **`rebase --root` without `--onto`:** synthetic empty-root commit (Git parity); disable preemptive fast-forward for that case.
- **`rebase --edit-todo`:** new flag for in-progress interactive rebase.
- **Final fixup in squash chain:** `is_final_fixup_in_todo` ignores trailing `exec` when looking for a following `fixup`/`squash`; final fixup with prior `squash` runs commit editor (`GIT_EDITOR`) and uses `prepare-commit-msg` source `squash`.
- **`pull.rs`:** `rebase::Args` initializer includes `edit_todo: false`.

## Validation

- `./scripts/run-tests.sh t3429-rebase-edit-todo.sh` — 7/7
- `cargo test -p grit-lib --lib` — pass
- `cargo check -p grit-rs` — pass
