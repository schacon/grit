# t9330-add-update-all — verbose on stderr

## Failure

Tests 14–15: `grit add -v` / `--verbose` with `2>actual` expected non-empty stderr; progress used `println!` (stdout).

## Fix

`grit/src/commands/add.rs`: route dry-run and verbose `add`/`remove` progress lines through `eprintln!` (including `update_tracked`, `stage_gitlink`, `stage_file` dry-run path).

## Verification

- `./scripts/run-tests.sh t9330-add-update-all.sh` — 26/26
- `cargo test -p grit-lib --lib` — pass
