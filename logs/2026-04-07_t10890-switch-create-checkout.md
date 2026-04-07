# t10890-cherry-pick-message

## Failure

Test 26 `cherry-pick works on a freshly created branch` runs
`grit switch -c pick-target <initial_oid>`. `switch` delegates to `checkout` with
`rest = ["-c", "pick-target", "<oid>"]`. Checkout only peeled `-b`/`-B` from
`rest`, so `-c` was treated as a revision and failed with `unknown revision: '-c'`.

## Fix

Extend checkout’s post-processing of `rest` to recognize `git switch` spellings:
`-c`/`--create` → same as `-b`, and `-C`/`--force-create` → same as `-B`.

## Verification

- `./scripts/run-tests.sh t10890-cherry-pick-message.sh` → 30/30 pass
- `cargo test -p grit-lib --lib` → pass
