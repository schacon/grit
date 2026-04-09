# check-ref-format --branch: reject `HEAD`

## Change

Git's `check_branch_ref` splices `refs/heads/` and rejects when the full ref is exactly `refs/heads/HEAD`. Grit previously accepted `grit check-ref-format --branch HEAD` while `/usr/bin/git` fails.

## Fix

- In `--branch` mode, exit 1 (no output) when the validated shorthand is `HEAD`, or when `@{-N}` resolves to `HEAD`.

## Validation

- `./scripts/run-tests.sh t9440-check-ref-format-branch.sh` — 34/34
- Manual: `grit check-ref-format --branch HEAD` now exits 1 like git
- `cargo test -p grit-lib --lib`
