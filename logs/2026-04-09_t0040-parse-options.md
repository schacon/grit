# t0040-parse-options

## Summary

Made `tests/t0040-parse-options.sh` fully pass (94/94).

## Changes

### grit-lib (`parse_options_test_tool`)

- **`run_parse_options`**: Scan full argv like Git—queue non-option args, handle lone `+` as NODASH countup, append remaining args after `--` / `end-of-options` / lone `-`.
- **`append_usage_if_unknown`**: Append full help only for unknown long/short options and ambiguous abbreviations (not for missing values, `takes no value`, ranges, etc.).
- **`long_exact` / `yes` and `doubt`**: Error messages use `no-yes` / `no-doubt` when unset, matching Git `optname()` for `OPT_UNSET`.

### grit binary (`main.rs`)

- Flush stdout/stderr before `process::exit` for `parse-options`, `parse-options-flags`, `parse-subcommand` (block-buffered redirect to files dropped completion line).
- Do not run the global clap `--git-completion-helper` path when the flag appears after a `test-tool` subcommand (e.g. `parse-subcommand cmd --git-completion-helper`).

### tests harness (`test-lib.sh`)

- **`test_run_`**: Stop wrapping test bodies in `$(printf ...)`, which executed backticks inside heredoc source and broke tests after the first block using `` `...' `` in expect lines.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t0040-parse-options.sh` → 94/94
