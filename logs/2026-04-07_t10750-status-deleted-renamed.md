# t10750-status-deleted-renamed

## Goal

Make `tests/t10750-status-deleted-renamed.sh` pass (40/40).

## Failure

Test 12 (`status -s shows old and new filename`) failed: after `grit mv alpha.txt renamed.txt`, `grit status -s` printed `R  renamed.txt` only. Git prints `R  alpha.txt -> renamed.txt` (and porcelain v1 short format matches).

## Fix

In `grit/src/commands/status.rs`, `format_short` now:

- For `Renamed` / `Copied` entries, prints `old -> new` when not `-z`.
- For `-z`, prints destination path then source path, each NUL-terminated, after `XY `, matching `wt_shortstatus_status` in Git’s `wt-status.c`.

## Verification

- `./scripts/run-tests.sh t10750-status-deleted-renamed.sh` → 40/40
- `cargo test -p grit-lib --lib` → pass
