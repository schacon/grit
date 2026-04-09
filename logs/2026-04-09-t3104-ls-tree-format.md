# t3104-ls-tree-format

- Implemented Git-compatible `ls-tree --format` expansion in `grit/src/commands/ls_tree.rs`:
  - `%(objectsize)`, `%(objectsize:padded)`, `%%`, `%n`, `%xNN`
  - `%(objectname)` uses `abbreviate_object_id` when `--abbrev` is set (matches `--object-only --abbrev`)
  - `%(path)` uses the same C-style quoting as `--name-only` when not `-z`
- Default/long `ls-tree` output now uses unique abbreviation for OIDs when `--abbrev` is set.
- Verified: `./scripts/run-tests.sh t3104-ls-tree-format.sh` → 19/19 pass.
