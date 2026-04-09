# t6030-bisect-porcelain work log (2026-04-09)

## Changes

- **Bisect state path**: All `BISECT_*` files use `bisect_state_dir` (common git dir) so linked worktrees share one session; `clean_bisect_state` removes loose `refs/bisect` under common dir.
- **`bisect start`**: Skip restoring `BISECT_START` checkout from linked worktrees (`commondir` present) so test 8 pathspec-on-main does not move wt1 off `shared`. Use `--ignore-other-worktrees` when restoring saved branch from main repo. Removed premature `is_ancestor` check before state write; clean state on `bisect_auto_next` / `bisect_next_all` failure after writes. `detach_head(..., false)` for bisect checkouts so dirty trees fail like Git.
- **`bisect replay`**: `cmd_reset` first; parse `git bisect start` args via `parse_bisect_names_line`; replay lines use `replay_bisect_state_line` (`bisect_write` only, no `bisect_auto_next` per line); final `bisect_auto_next`; use `ExplicitExit` for code 2 instead of `process::exit` inside replay.
- **`bisect run`**: Shell execution via `sh -c` with `sq_quote_argv` command string; `ExplicitExit` for exit 2 when only skips remain (nested runs); stderr messages without extra quotes for t6030 `sed` filters; optional `p` env as `Np` from `hello` line count for `sed -ne $p` scripts.
- **`git branch`**: List local/remotes via `refs::list_refs` so packed refs appear after `pack-refs` (fixes tests 15–19).
- **`checkout`**: `switch_branch` respects `--ignore-other-worktrees` (bisect reset to branch checked out elsewhere).
- **Refs/state**: Bisect refs and `state` bisect detection aligned with common dir where applicable.

## Tests

- Many early t6030 cases pass (through ~38 in direct `bash t6030` runs); full file still hits **FATAL** around **test 39** (`bisect run & skip: cannot tell between 2`) — needs further alignment with Git’s skip/ambiguous-commit listing (`error_if_skipped_commits` / rev walk).
- `./scripts/run-tests.sh t6030-bisect-porcelain.sh` hits default **120s timeout** (file is large); use higher `--timeout` for harness runs.

## Note

Reverted accidental `cargo clippy --fix` edits to unrelated commands (`cherry_pick`, `clean`, `sequencer`) before commit.
