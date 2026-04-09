# t4137-apply-submodule (partial)

## Changes

- `apply`: merge adjacent delete-blob + add-gitlink patches (file → submodule); extend gitlink index handling for gitlink↔gitlink updates; remove blocking files before submodule placeholder dirs; `--3way` flag (accepted; full three-way merge not wired); `ensure_gitlink_placeholder_dirs` replaces mistaken files with dirs.
- `checkout`: stop spawning unconditional `submodule update --init` after every `checkout_index_to_worktree` (was rewriting index vs HEAD after checkouts like `replace_sub1_with_file`).

## Harness

- `./scripts/run-tests.sh t4137-apply-submodule.sh` was at **10/28** passing at commit time; remaining failures include `apply_3way` cases and some `apply_index` submodule removal / conflict expectations.

## Note

`AGENTS.md` forbids GitButler MCP; no `update_branches` run.
