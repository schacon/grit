# t8003-blame-corner-cases

## Summary

Made `tests/t8003-blame-corner-cases.sh` pass 30/30.

## Root causes

1. **`resolve_blame_start_oid`**: A second `split_once("..")` block treated `HEAD^` as `HEAD` + `^`, resolving to the tree OID instead of the parent commit. Removed the dead/wrong branch (first `if` already handled `..` ranges).

2. **`parse_blame_args` (two positional args)**: `resolve_revision` index DWIM and short-hex disambiguation could resolve a filename like `f` to a blob OID, so the parser swapped rev/path (`HEAD^` became the path). Fixed by resolving without index DWIM and treating a token as the revision only when it peels to a commit via `peel_to_commit_oid`; if both peel to commits, Git order (`rev` then `path`) is used.

## Validation

- `./scripts/run-tests.sh t8003-blame-corner-cases.sh` → 30/30
- `cargo fmt`, `cargo check -p grit-rs`, `cargo clippy -p grit-rs --fix --allow-dirty`
- `cargo test -p grit-lib --lib`
