# t6408-merge-up-to-date

## Problem

`merge -s ours c1` with HEAD at `c0` (ancestor of `c1`) incorrectly fast-forwarded to `c1`, so `HEAD^{tree}` matched `c1` instead of staying on `c0`'s tree. Upstream Git records a merge commit with two parents whose tree is ours.

## Fix

In `merge.rs`, before `do_fast_forward` when `is_ancestor(head, merge_target)`, if the strategy is exactly `-s ours`, call `do_strategy_ours` (after the same index check as other merge paths) instead of fast-forwarding.

## Verification

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t6408-merge-up-to-date.sh` → 7/7 pass
