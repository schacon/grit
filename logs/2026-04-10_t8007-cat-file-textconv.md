# t8007-cat-file-textconv

## Goal

Make `tests/t8007-cat-file-textconv.sh` pass (15/15).

## Changes

- **`grit-lib` `rev_parse::resolve_treeish_blob_at_path`**: Run `diagnose_tree_path_error` when tree walk fails so missing paths report `fatal: path '…' does not exist in 'HEAD'` like Git.
- **`grit` `cat_file`**:
  - For `--textconv` / `--filters`, resolve via `resolve_object_with_mode_lib` (index `:path`, `resolve_treeish_blob_at_path` for `rev:path`) and print `Error::Message` verbatim; `-e` exits 128 on those messages (matches Git).
  - Map ambiguous bare rev + transform mode to `fatal: Not a valid object name <rev>` (Git behavior for `cat-file --textconv HEAD2`).
  - Symlink blobs (`MODE_SYMLINK`): skip `run_textconv_raw` in single-object and `--batch` paths; output smudged symlink target bytes.
- **`PLAN.md` / `progress.md`**: Mark t8007 complete; bump counts.
- **Harness**: `./scripts/run-tests.sh t8007-cat-file-textconv.sh` → 15/15, CSV/dashboards updated.

## Validation

- `cargo fmt`, `cargo clippy --fix --allow-dirty` (workspace scope as run)
- `cargo test -p grit-lib --lib`
- `GUST_BIN=... sh tests/t8007-cat-file-textconv.sh`
