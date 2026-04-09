# t3650-replay-basics

## Summary

Implemented Git-compatible `grit replay` behavior needed for `tests/t3650-replay-basics.sh`:

- CLI: `--onto`, `--advance`, `--contained`, `--ref-action`, `--branches`, `--ancestry-path`
- Revision list via `rev_list` (topo + reverse), including `^!` expansion for single commits
- `--branches` passes full `refs/heads/*` names so `--onto` mode satisfies “all positives are references”
- Ref updates: print `update <ref> <new> <old>` or apply with `replay --onto <hex>` / `replay --advance <branch>` reflog messages
- `expand_rev_token_circ_bang` in `grit-lib` for `topic1^!` style specs (reject merges like Git)

Fixed `git log --format=%s%d`: templates with `%d` now use **short** ref decorations by default (Git behavior); full names only with `--decorate=full`.

## Validation

- `./scripts/run-tests.sh t3650-replay-basics.sh` — 31/31
- `cargo test -p grit-lib --lib` — pass

## Note

Workspace `cargo clippy -- -D warnings` currently fails on many pre-existing `grit-lib` lints; not addressed in this change.
