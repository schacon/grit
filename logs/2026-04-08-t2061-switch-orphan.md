# t2061-switch-orphan

## Problem

`git switch --orphan` delegated to checkout but behaved like `checkout --orphan`: index/worktree kept prior branch content, extra start-point was accepted, existing branch ref was deleted, and `--discard-changes` did not force because `-f` was peeled from `rest` after `switch_force` was computed.

## Fix

- `switch.rs`: reject combining `--orphan` with `-c`/`-C` (fatal 128, matches Git).
- `checkout.rs`: `create_orphan_branch` takes `CreateOrphanOptions { switch_style, force }`.
  - **switch_style** (`switch_mode`): empty tree checkout via `switch_to_tree`, unborn HEAD, reject `<start-point>`, reject existing branch name (no ref delete).
  - **checkout** path unchanged for index preservation / start-point behavior.
- Compute `switch_force` after peeling `-f` from `rest` so `switch --discard-changes --orphan` passes force through.

## Validation

- `./scripts/run-tests.sh t2061-switch-orphan.sh` — all pass.
- `t2017-checkout-orphan.sh` — unchanged vs baseline (test 8 reflog still fails pre-existing).
