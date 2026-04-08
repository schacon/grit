# t0023-crlf-am — diff-files false positive

## Problem

`git am -3` with `core.autocrlf true` left new index entries with zeroed stat metadata (ctime/mtime/dev/ino). `grit diff-files` treated those as smudged and reported `M` even when the worktree blob OID matched the index OID, so `git diff-files --name-status --exit-code` failed.

Upstream Git 2.43 still reports clean in that scenario (worktree may hold CRLF while the index stores LF; hashing uses clean conversion).

## Fix

In `grit/src/commands/diff_files.rs`, removed the `is_stat_smudged` branch from the stage-0 change detection. If stat matched enough to take the fast path, or a full read+hash shows the same OID and mode as the index, the file is clean regardless of zeroed index timestamps.

## Validation

- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t0023-crlf-am.sh` → 2/2 pass
- `cargo test -p grit-lib --lib`
