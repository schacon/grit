# t11530-switch-orphan-track

## Problem

`grit switch --orphan` and `grit switch -d` passed those flags through `rest` to `checkout`; clap did not parse them, so `--orphan` / `--detach` were treated as revision names.

Orphan without a start point also left the previous branch’s index and tracked files in the worktree; Git clears both.

## Fix

- In `checkout::run`, after extracting `-b`/`-c` from `rest`, strip `--orphan <name>`, `--detach`, and `-d` into `args.orphan` / `args.detach`.
- In `create_orphan_branch`, when there is no start point and a work tree exists, sync to an empty index via `checkout_index_to_worktree` and write an empty index.

## Verification

- `./scripts/run-tests.sh t11530-switch-orphan-track.sh` — 30/30 pass
- `cargo test -p grit-lib --lib` — pass
