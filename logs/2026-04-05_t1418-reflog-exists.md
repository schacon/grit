# t1418-reflog-exists — Log

**Date:** 2026-04-05
**Branch:** fix/reflog-exists
**Status:** PASS (6/6 tests)

## Summary

All 6 tests in t1418-reflog-exists.sh were already passing with the current
implementation. No code changes were needed.

## Tests

1. `setup` — test_commit A — OK
2. `usage` — exit code 129 for missing args and -h — OK
3. `usage: unknown option` — exit code 129 for --unknown-option — OK
4. `reflog exists works` — refs/heads/main exists, nonexistent fails — OK
5. `reflog exists works with a "--" delimiter` — OK
6. `reflog exists works with a "--end-of-options" delimiter` — OK

## Implementation Notes

The `reflog exists` subcommand is implemented in:
- `grit/src/commands/reflog.rs` — ExistsArgs struct and run_exists handler
- `grit-lib/src/reflog.rs` — reflog_exists() checks file at git_dir/logs/<refname>

The `--end-of-options` flag is handled via clap's `#[arg(long)]` attribute.
The `--` delimiter is handled by clap automatically for positional args.
