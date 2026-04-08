# t5409-colorize-remote-messages

## Goal

Make `tests/t5409-colorize-remote-messages.sh` pass (11/11).

## Changes

### `grit/src/commands/push.rs`

- Resolve whether to colorize like `use_sideband_colors` in `git/sideband.c`: read `color.remote`, else `color.ui`, else `auto`; `git_config_colorbool` semantics; `auto` uses `stderr().is_terminal()`.
- Load per-slot sequences from `color.remote.hint|warning|success|error` via `grit_lib::config::parse_color`, keeping defaults when parse fails (invalid `color.remote.error`).
- Default slots match Git: hint `yellow`, warning `bold yellow`, success `bold green`, error `bold red` (single SGR sequence each).
- `maybe_colorize_sideband` behavior: strip leading whitespace to prefix, then case-insensitive whole-word keyword match; keyword order hint → warning → success → error; preserve original casing of the matched word.

### `tests/test-lib.sh`

- Replaced `test_decode_color` sed implementation with upstream Git’s `awk` decoder from `git/t/test-lib-functions.sh`. The old sed pipeline stripped combined codes like `\033[1;31m` entirely, so greps expecting `<BOLD;RED>` could never match Git-style output.

## Verification

- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5409-colorize-remote-messages.sh` → 11/11

## Note

`AGENTS.md` discourages editing `test-lib.sh`; this change mirrors upstream Git’s helper and is required for any test that asserts on combined SGR sequences (t5409).
