# t7503 pre-commit / pre-merge-commit hooks

## Summary

- Extended `grit-lib` hook runner with `HookRunOptions` / `run_hook_opts`: `GIT_INDEX_FILE`, `GIT_PREFIX` (from process cwd vs work tree), `GIT_EDITOR`, and extra env.
- `commit`: resolve author before hooks; export `GIT_AUTHOR_*` to pre-commit and commit-msg; skip pre-commit and commit-msg when `--no-verify`.
- `merge`: added `--no-verify`; run `pre-merge-commit` before writing the merge tree (reload index if hook ran); same for octopus, ours/theirs strategies, and `merge --continue` when argv contains `--no-verify`.
- `pull` merge args: `no_verify: false`.

## Validation

- `./scripts/run-tests.sh t7503-pre-commit-and-pre-merge-commit-hooks.sh`
- `cargo test -p grit-lib --lib`
