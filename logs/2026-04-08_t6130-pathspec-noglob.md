# t6130-pathspec-noglob

## Summary

Implemented Git-compatible pathspec globals and magic handling so `log`/`rev_list` path limiting matches upstream for glob vs literal, `:(glob)` / `:(literal)`, `**/`, and `GIT_LITERAL_PATHSPECS`.

## Changes

- Added `grit_lib::pathspec` with `pathspec_matches`, `simple_length`, `validate_global_pathspec_flags`.
- `apply_globals` in `grit` sets `GIT_LITERAL_PATHSPECS`, `GIT_GLOB_PATHSPECS`, `GIT_NOGLOB_PATHSPECS`, `GIT_ICASE_PATHSPECS` from CLI flags; added `--no-literal-pathspecs`.
- `grit::pathspec::pathspec_matches` delegates to the library; removed duplicate glob implementation.
- `rev_list` uses shared `crate::pathspec::pathspec_matches`.

## Verification

- `cargo test -p grit-lib --lib`
- `GUST_BIN=... sh tests/t6130-pathspec-noglob.sh` — 21/21 pass
- `./scripts/run-tests.sh t6130-pathspec-noglob.sh`
