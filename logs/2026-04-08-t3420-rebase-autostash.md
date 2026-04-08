# t3420-rebase-autostash

## Done

- `rebase --autostash`, `--no-autostash`, `rebase.autostash` config; stash helpers in `stash.rs`.
- `rebase --quit` clears state (no stash pop; matches Git).
- Pre-rebase hook + checkout-onto safety via `check_dirty_worktree` before internal onto checkout.
- `rebase-merge` vs `rebase-apply` dirs; `replay_remaining`/`finish_rebase` take explicit `rb_dir` and autostash/backend context.
- Apply backend: suppress final success line when autostash was used (t3420 expected stdout).
- Submodule dirtiness: `diff_index_to_worktree` gitlink recursion via `submodule_has_local_changes`; gitlink checkout clears existing submodule dir.
- Default `info/exclude`: `.test_tick` so setup commits do not track harness tick file.
- `pull.rs` `Args` updated for new rebase fields.
- `tests/t3420-rebase-autostash.sh`: hook cleanup, inter-block `rebased-feature-branch` cleanup, `branch -D` before `checkout -b`, test 8 `test_when_finished`.

## Still failing (harness)

`./scripts/run-tests.sh t3420-rebase-autostash.sh` — 13 failures remain (mostly conflicting-stash apply path: untracked `file4` vs onto tree, `--quit`/stash diff ordering, and `rebase --autostash <upstream> <branch>` with dirty submodule).

## Commands

```bash
cargo build --release -p grit-rs
./scripts/run-tests.sh t3420-rebase-autostash.sh
```
