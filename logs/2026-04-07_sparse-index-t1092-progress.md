# Sparse index / t1092 progress (2026-04-07)

## Done

- Index: parse/write `sdir` extension; sparse directory placeholders (`MODE_TREE` + skip-worktree); expand via ODB; collapse using cone patterns and nested sparse-dir awareness.
- `Repository::load_index` / `load_index_at` expand placeholders; `write_index` / `write_index_at` re-collapse before write.
- Wired most command paths to expanded load + finalized write (reset, checkout, add, merge, etc.).
- `sparse-checkout`: `init`/`set`/`reapply`/`add` flags (`--cone`, `--no-cone`, `--sparse-index`, `--no-sparse-index`, `--skip-checks`); `index.sparse` config; collapse after apply.
- `ls-files --sparse` reads on-disk index without expanding.
- `status`: sparse-checkout banner (percentage vs sparse-index one-liner).
- `write_tree_from_index`: ignore `MODE_TREE` entries.
- `rev_parse` index path lookup uses expanded index.
- Tests: `test_cmp_config` aligned with upstream Git (`git -C … config` + file compare); added `test_region` for trace2 checks in t1092.

## Still failing

- `./scripts/run-tests.sh t1092-sparse-checkout-compatibility.sh` — ~65 failures remain (2 expected `test_expect_failure`).

## Next steps (for a follow-up)

- Remaining t1092 failures likely need command-specific sparse behavior (blame outside cone, reset pathspecs, read-tree, submodule paths, etc.).
