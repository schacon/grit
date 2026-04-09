# t4027-diff-submodule

## Summary

Made `t4027-diff-submodule` pass (20/20) by aligning submodule raw diff, ignore rules, combined conflict output, and harness `test_oid` fallback.

## Changes

- **grit-lib `diff`**: Gitlink tree↔worktree uses null new OID when submodule HEAD differs; emit same-OID modified when HEAD matches but worktree dirty; `hash_worktree_file` treats directories as empty bytes (EISDIR).
- **grit `diff`**: Raw mode for same-OID gitlink; patch headers resolve submodule HEAD when new side is null; `diff.ignoreSubmodules` + `.gitmodules` + CLI `--ignore-submodules` filtering and `-dirty` suffix; unmerged index → `--cc` without `MERGE_HEAD`; `format_gitlink_unmerged_conflict_combined` for synthetic OIDs.
- **grit `diff-index` / `diff-files`**: Gitlink raw OIDs and dirty-aligned index cases.
- **grit `config`**: `git config --add -f file key val` when `--add` has no inline key.
- **tests `test-lib.sh`**: `test_oid` fallback reads `GIT_SOURCE_DIR/t/oid-info/oid` (e.g. `ff_1`).
- **Harness**: `run-tests.sh` refreshed CSV + dashboards.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t4027-diff-submodule.sh` → 20/20
