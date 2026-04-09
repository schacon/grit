# t3902-quoted — ls-files with core.quotepath false

## Issue

Harness `t3902-quoted.sh` failed test 9: after `git config --bool core.quotepath false`, `git ls-files` output did not match `expect.raw` (paths with tabs/LF/double-quote still needed C-style escapes; UTF-8 names should stay literal).

## Root cause

`format_ls_path` in `grit/src/commands/ls_files.rs` returned the raw path whenever `quote_fully` was false, skipping `quote_c_style` entirely. Diff/ls-tree already called `quote_c_style` with `quote_fully` from config; ls-files was wrong.

## Fix

Only skip quoting for `-z` (nul-terminated) output. Otherwise always use `quote_c_style(name, quote_fully)` so `core.quotepath false` matches Git (escape ASCII specials only, not octal UTF-8).

## Validation

- `sh ./tests/t3902-quoted.sh` — 13/13 pass
- `cargo test -p grit-lib --lib` — pass
- `./scripts/run-tests.sh t3902-quoted.sh` — updates CSV to 13/13
