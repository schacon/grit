# t2200-add-update: dry-run output on stdout

## Failure

Test 16 `add -n -u should not add but just report` compared `git add -n -u >actual` to an expected file with `add 'check'` and `remove 'top'`. Grit printed those lines on stderr via `eprintln!`, so stdout was empty and `test_cmp` failed.

## Fix

In `grit/src/commands/add.rs`, use `println!` for dry-run `add`/`remove` messages in `update_tracked` and in `stage_file` when `ctx.dry_run` (matches Git: informational dry-run lines go to stdout).

## Verification

- `./scripts/run-tests.sh t2200-add-update.sh` → 19/19 pass
- `cargo test -p grit-lib --lib` → pass
