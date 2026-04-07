# t5405-send-pack-rewind

## Symptom

Harness failed at setup: `git fetch --update-head-ok .. main:main` in clone `another/` errored with refusal to update `refs/heads/main` while checked out.

## Fix

In `grit/src/commands/fetch.rs`, skip the `is_branch_in_worktree` check when `args.update_head_ok` is true (CLI already had `--update-head-ok`); applied for both glob-refspec and single-refspec update paths.

## Validation

- `GUST_BIN=... sh tests/t5405-send-pack-rewind.sh` — 3/3
- `./scripts/run-tests.sh t5405-send-pack-rewind.sh` — 3/3
- `cargo fmt`, `cargo check -p grit-rs`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`
