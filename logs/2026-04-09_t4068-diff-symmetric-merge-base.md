# t4068-diff-symmetric-merge-base

## Fixes

1. **merge**: `resolve_merge_target` now uses `resolve_revision_as_commit` so annotated tags peel to commits (fixes corrupt-commit errors when merging a tag like `commit-C`).
2. **diff / diff-index / diff-tree**: Implemented `--merge-base` and Git-style symmetric-diff argument rules (`A...B` warning on multiple bases, usage errors for mixed ranges, `fatal: …: no merge base`).
3. **Tag peeling**: `commit_or_tree_oid` (diff) and `resolve_to_tree` (diff-tree) peel annotated tag chains so `commit-C` and similar resolve like Git.
4. **diff-index**: `--merge-base` uses merge-base(HEAD, arg) tree; non-commit tree-ish reports `fatal: <spec> is a tree, not a commit`.

## Validation

- `./scripts/run-tests.sh t4068-diff-symmetric-merge-base.sh` — 36/36 pass.
