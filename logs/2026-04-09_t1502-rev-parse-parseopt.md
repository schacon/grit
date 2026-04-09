# t1502-rev-parse-parseopt

## Summary

Implemented `git rev-parse --parseopt` to match Git’s `cmd_parseopt` / `parse_options` behavior so `tests/t1502-rev-parse-parseopt.sh` passes 37/37.

## Changes

- New module `grit/src/commands/rev_parse_parseopt.rs`: read usage + options from stdin until `--`, parse option lines like Git (`OPTION_GROUP`, flags `=!?*`, short/long names), emit help with `cat <<\EOF` wrapper on success paths, plain usage on stderr for unknown/ambiguous options (no heredoc wrapper).
- Parse argv after `--` with support for `--keep-dashdash`, `--stop-at-non-option`, `--stuck-long`; long/short abbreviation and ambiguity; `GIT_TEST_DISALLOW_ABBREVIATED_OPTIONS`.
- Synthetic hidden `no-<name>` entries for each negatable long option so `--no` can match both `noble` and `no-noble` (t1502 ambiguous case).
- `rev_parse.rs` delegates to the new module.

## Validation

- `./scripts/run-tests.sh t1502-rev-parse-parseopt.sh` → 37/37
- `cargo test -p grit-lib --lib`
- `cargo check -p grit-rs`
