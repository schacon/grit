# t8010-cat-file-filters

## Goal

Make `tests/t8010-cat-file-filters.sh` pass (9/9).

## Changes

- **`grit-lib`**: `merge_diff::run_textconv_raw` + `convert_blob_to_worktree_for_path` (reuse checkout smudge/EOL pipeline with optional index for `.gitattributes`).
- **`grit cat-file`**: Non-batch `--filters` / `--textconv` output; `--batch` + `--textconv`/`--filters` with first-token OID + optional path suffix; size line uses stored blob size, content line uses transformed bytes; Git-compatible errors (`missing path`, bare `<rev> required`, `--batch-all-objects` incompatibility).

## Verification

```bash
cargo build --release -p grit-rs
./scripts/run-tests.sh t8010-cat-file-filters.sh
cargo test -p grit-lib --lib
```

All green.
