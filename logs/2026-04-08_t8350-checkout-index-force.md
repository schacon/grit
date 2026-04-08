# t8350-checkout-index-force

## Issue

Harness showed 28/30 passing: (1) exit code 1 when skipping an existing dirty file without `--force` (test expected 0); (2) `--no-create --all` then `checkout-index --all` failed because unchanged files still had stale index stat cache vs disk.

## Fix (`grit/src/commands/checkout_index.rs`)

- Removed treating “skipped existing file” as a fatal conflict (`had_conflict` + `exit(1)`). Git returns 0 when checkout is skipped.
- When a path exists and `--force` is off: if symlink target matches index blob, or cached stat matches, or cleaned worktree content hashes to the index OID (Git-style stat/content match), treat as up to date with no warning; otherwise warn and skip without failing.

## Verification

- `./scripts/run-tests.sh t8350-checkout-index-force.sh` → 30/30.
