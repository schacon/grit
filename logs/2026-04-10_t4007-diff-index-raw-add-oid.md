# t4007-rename-3: diff-index raw OID for uncached adds

## Problem

Tests 7 and 13 failed: pathspec-limited `git diff-index -C --find-copies-harder <tree> path1`
expected `:000000 100644 $ZERO_OID $blob A	path1/COPYING` (index blob on the new side).

Grit always printed zeros for both OIDs on uncached adds in raw output.

## Fix

In `grit/src/commands/diff_index.rs`, `render_raw_diff_entry` and `write_raw_diff_entry_z`:
for uncached `DiffStatus::Added`, keep old side as zeros; show abbreviated/full `new_oid` when
non-zero (index matches work tree), else zeros (work tree differs; placeholder like Git).

## Validation

- `./scripts/run-tests.sh t4007-rename-3.sh` — 13/13 pass
- `cargo test -p grit-lib --lib` — pass
- `cargo check -p grit-rs` — pass
