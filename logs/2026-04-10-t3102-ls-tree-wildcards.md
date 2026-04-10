# t3102-ls-tree-wildcards

## Goal

Make `tests/t3102-ls-tree-wildcards.sh` pass (4/4).

## Changes

- **`grit-lib/pathspec`**: Added `matches_ls_tree_pathspec` (Git `ls-tree` after it clears `has_wildcard`: no `*`/`?` → literal match so `a[a]` works; with `*`/`?` → `simple_length` + `wildmatch` tail). Added `pathspec_is_exclude_only`, `matches_pathspec_set_for_object_ls_tree`, `pathspec_wants_descent_into_tree`, and `matches_pathspec_set_for_object` for combined positive + `:(exclude)` lists (archive/update-index keep using per-spec `matches_pathspec_for_object`).
- **`grit ls-tree`**: Pathspec filtering via `matches_pathspec_set_for_object_ls_tree` + `.gitattributes` load; skip tree narrowing when pathspecs present (full tree walk like Git `read_tree`); cwd-relative display still uses `cwd_rel` as prefix for `../` paths outside cwd.
- **`grit ls-files`**: Combined pathspec matching with excludes via `matches_pathspec_set_for_object_ls_tree`; mark exclude-only specs satisfied for `--error-unmatch`; same for `--others` untracked paths.

## Verification

- `./scripts/run-tests.sh t3102-ls-tree-wildcards.sh` → 4/4
- `cargo test -p grit-lib --lib`
