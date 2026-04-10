# t3438-rebase-broken-files

## Summary

Made harness file `tests/t3438-rebase-broken-files.sh` fully pass (9/9).

## Changes

- **`grit/src/commands/rebase.rs`**
  - On merge-backend conflict with `rebase-merge/`, write `author-script` from the picked commit’s author line (Git `write_author_script` format: `GIT_AUTHOR_NAME` / `GIT_AUTHOR_EMAIL` / `GIT_AUTHOR_DATE` with shell single-quoting).
  - `rebase --continue` for the normal pick path reads and strictly validates `author-script` (missing keys, duplicates, unknown keys, bad quoting) and builds the replayed commit author from the parsed line; falls back to original commit metadata when the file is absent.
  - Apply backend: before starting rebase, try create/truncate `.git/rebased-patches` and remove it; on `PermissionDenied`, fail early with Git-style “could not open … for writing” so no `rebase-apply` state leaks (t3438 last test).

## Validation

- `cargo build --release -p grit-rs`
- `cargo test -p grit-lib --lib`
- Manual: `bash tests/t3438-rebase-broken-files.sh` (all ok)
- `./scripts/run-tests.sh t3438-rebase-broken-files.sh`

## Note

Workspace `cargo clippy -- -D warnings` currently reports many pre-existing issues in other crates; scoped check of `rebase.rs` via IDE lints is clean.
