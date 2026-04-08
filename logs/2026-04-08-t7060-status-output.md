# t7060-status-output

## Issue

Harness reported 24/25: `porcelain on clean repo shows only branch header` failed because `git status --porcelain` emitted no `##` line on a clean repo.

## Fix

In `grit/src/commands/status.rs`, record whether `--porcelain` was passed before `-z` may synthesize porcelain mode. When the user explicitly requests porcelain v1, force branch header output (unless `--no-branch`), matching the harness note that grit always includes `##` for `--porcelain`. `-z` alone still does not add `##` without `-b`.

## Verification

- `./scripts/run-tests.sh t7060-status-output.sh` → 25/25
- `cargo test -p grit-lib --lib`
- `cargo check -p grit-rs`
