# t10560-switch-create-detach

## Goal

Make `tests/t10560-switch-create-detach.sh` pass (28 tests).

## Failures (before)

1. `switch -- file1.txt-branch` — checkout path treated `file1.txt-branch` as a pathspec (ambiguous with branch name containing `.`).
2. `switch -c "bad branch name"` — should fail; grit accepted invalid branch names.

## Changes

- `checkout::Args.switch_mode` (hidden `--__grit_switch_mode`): `switch` sets this when delegating to checkout.
- `split_target_and_paths`: when `switch_mode && has_separator && !separator_at_end && rest.len() == 1`, treat the lone arg as the switch target (branch/commit), not pathspecs.
- `validate_new_branch_name`: `check_refname_format` with `allow_onelevel: true`; used in `create_and_switch_branch`, `force_create_and_switch_branch`, `create_orphan_branch`.

## Verification

- `cargo fmt`, `cargo check -p grit-rs`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t10560-switch-create-detach.sh` → 28/28 after release build
