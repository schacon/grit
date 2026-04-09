# t3100-ls-tree-restrict

## Issue

`ls-tree -t` with a pathspec that names a missing path under an existing prefix (e.g. `path2/bak`) must still print intermediate trees walked while searching (Git: `show_recursive` + `LS_SHOW_TREES`). Grit descended but omitted the `path2` line.

## Fix

In `grit/src/commands/ls_tree.rs`, when descending into a tree for pathspec prefix matching without `-r`, emit the tree entry first if `--show-trees` (`-t`) is set.

## Validation

- `./scripts/run-tests.sh t3100-ls-tree-restrict.sh` → 14/14
- `cargo check -p grit-rs`, `cargo test -p grit-lib --lib`
