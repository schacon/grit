# t0040-parse-options fix

## Summary

Aligned `test-tool parse-options` with Git `parse-options.c` behavior so `tests/t0040-parse-options.sh` passes 94/94.

## Changes

- Short options: explicit letters win over `OPTION_NUMBER` digit runs (`-b-4` → boolean then bit, not `-b` + `-4` as number).
- Typo detection: `check_typos` on full cluster after `-` (e.g. `-boolean`, `-ambiguous`); prefix match uses `starts_with(long_name, user)` semantics.
- `--no-list` clears list without requiring `=`; quiet uses positive count-up like Git.
- `--no-length`: callback error exits 1 with empty stderr (`ParseOptionsToolError::Silent`).
- Usage appended after unknown-option errors: no extra blank line before help text.

## Validation

- `cargo test -p grit-lib --lib`
- `cargo clippy -p grit-lib -p grit-rs`
- `./scripts/run-tests.sh t0040-parse-options.sh` → 94/94
