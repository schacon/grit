# t13320-mv-case-sensitive

## Context

Harness was already 30/30 on Linux; hardened behavior for case-insensitive filesystems (macOS/Windows) where `rename(a, A)` can fail when source and destination are the same directory entry.

## Changes

- **`grit init`**: After creating `config`, if `.git/CoNfIg` is accessible (Git’s probe), set `core.ignorecase = true` (matches `git/setup.c`).
- **`grit mv`**: For ASCII case-only renames, when `core.ignorecase` is true or Unix metadata shows `src` and `dst` are the same inode, rename via a unique intermediate path under the destination parent.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t13320-mv-case-sensitive.sh` → 30/30
