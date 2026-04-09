# t7407-submodule-foreach

## Summary

Made `tests/t7407-submodule-foreach.sh` pass (23/23).

## Changes

- **`grit submodule foreach`**: Git-compatible env (`toplevel`, `path`, `sha1`, `displaypath` from cwd), sorted traversal, recursive nested superproject context, stdout "Entering" lines, `-q`, optional command default `:`, `--` validation, single-arg vs multi-arg shell execution (quoted vs `exec "$@""`).
- **`grit submodule status`**: `--cached`, `--recursive`, path filtering under prefix, display paths relative to invocation cwd, parentheticals via `grit describe` subprocess (same attempts as Git’s `compute_rev_name`).
- **`grit submodule update`**: `--reference` (alternates + propagation to nested updates).
- **`grit pull`**: detached HEAD uses remote’s default branch for local path remotes.
- **`grit describe`**: `--contains` includes lightweight tags (for `file2~1`-style names).
- **`grit rev-parse --resolve-git-dir`**: do not exit early so later args still run (matches `d` noise arg in tests).
- **`tests/test-lib.sh`**: `test_grep` passes pattern with `grep -e` so patterns like `--quiet` are not treated as grep options.

## Validation

- `cargo build --release -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7407-submodule-foreach.sh` → 23/23
