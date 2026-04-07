# t8150-config-multivar — `config unset --all` section header

## Issue

Test `config unset --all leaves section header behind` failed: after removing all
values for a key, `[ns]` was stripped from `.git/config`.

## Cause

`ConfigFile::unset_matching` always called `remove_empty_section_headers()`, which
matches Git for legacy `--unset` / `--unset-all` but not for the new subcommand
`git config unset --all`, which leaves an empty section header.

## Fix

- Added `preserve_empty_section_header` to `unset_matching`.
- `cmd_unset` passes `true` for `config unset --all` only; legacy paths pass `false`.

## Validation

- `./scripts/run-tests.sh t8150-config-multivar.sh` → 29/29
- `cargo test -p grit-lib --lib`
