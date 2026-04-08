# t7509-commit-authorship

## Goal

Make `tests/t7509-commit-authorship.sh` pass (12/12) under the harness.

## Changes

- `grit/src/commands/commit.rs`: Git-compatible author resolution for `-C`/`-c` with `--reset-author` (reuse message only); read author from `CHERRY_PICK_HEAD` when present unless `--reset-author`; `--author` parses `Name <email>` and appends timestamp like Git; validate amended HEAD author for empty/missing date; allow `--reset-author` when `CHERRY_PICK_HEAD` or `REBASE_HEAD` exists; error messages for incompatible `--reset-author`/`--author` and invalid `--reset-author` context.

## Verification

- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t7509-commit-authorship.sh` → 12/12
- `cargo test -p grit-lib --lib`
- `cargo clippy -p grit-rs --fix --allow-dirty`
