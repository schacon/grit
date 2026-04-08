# t3060-ls-files-with-tree

## Goal

Make `tests/t3060-ls-files-with-tree.sh` pass (8 tests): `git ls-files --with-tree`.

## Changes

- **`grit-lib`**: `Index::overlay_tree_on_index` mirrors Git `overlay_tree_on_index`: shift existing stages 1–3 to stage 3, walk the named tree (respecting common pathspec prefix), add synthetic stage-1 entries, mark duplicates of stage 0 with an in-memory extended flag so output skips them (`overlay_tree_skip_output`).
- **`grit` `ls-files`**: `--with-tree`, `--recurse-submodules`; reject incompatible combinations with `fatal:` messages (exit 128 via main’s `fatal:` handling); strip trailing slash from overlay prefix to match Git `get_common_prefix_len`.
- **Pathspec**: literal specs with trailing `/` (cwd prefix) now match paths under that directory (e.g. `sub/` matches `sub/file`), fixing listing from a subdirectory.

## Verification

- `bash tests/t3060-ls-files-with-tree.sh` with `GUST_BIN=tests/grit`: 8/8 pass.
- `cargo test -p grit-lib --lib`: pass.

## Note

Workspace `cargo clippy -- -D warnings` reports many pre-existing issues; only touched files were lint-clean via IDE diagnostics.
