# t4213-log-tabexpand

## Goal

Make `tests/t4213-log-tabexpand.sh` pass (Git `log`/`show` commit message tab expansion).

## Changes

- Added `grit_lib::tab_expand`: `expand_tabs_in_line`, `expand_tabs_in_multiline_message`, `indent_and_expand_tabs`, `default_expand_tabs_for_pretty_format`, `resolve_expand_tabs_in_log` (matches Git `pretty.c` / `revision.c` defaults).
- `show` + `log`: `--expand-tabs[=N]`, `--no-expand-tabs`; resolve effective width after pretty alias resolution.
- `main.rs`: preprocess bare `--expand-tabs` → `--expand-tabs=8` for `log` and `show`.
- `log`/`show` `--format` / `--pretty`: `num_args=0..=1`, `default_missing_value=medium` so `git show -s --pretty` works like Git.
- Extended `log` pretty branches: `email`, `raw` (previously fell through to `%` format parsing).
- Reflog walk + notes lines use the same indent + tab expansion.

## Validation

- `./scripts/run-tests.sh t4213-log-tabexpand.sh` → 9/9
- `cargo fmt`, `cargo clippy --fix --allow-dirty`, `cargo test -p grit-lib --lib`
