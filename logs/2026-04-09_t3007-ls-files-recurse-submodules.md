# t3007-ls-files-recurse-submodules

## Goal

Make `tests/t3007-ls-files-recurse-submodules.sh` fully pass (24/24).

## Changes

- **`grit-lib`**
  - `submodule_config`: parse `.gitmodules` → path/name, implement Git-style `is_submodule_active` (`submodule.<name>.active`, `submodule.active`, URL fallback).
  - `ConfigSet::load_with_options`: merge `config.worktree` only when `extensions.worktreeConfig` is enabled in the common repo `config` (matches Git; fixes t3007 #19).
  - `Repository::load_index_at`: reject invalid `index.sparse` boolean so submodule config errors surface during recurse (t3007 #17–18).
  - `pathspec`: `path_allowed_by_pathspec_list`, `pathspec_exclude_matches`, `pathspec_contributes_match` for `:(exclude)` + icase combos (t3007 #12).
- **`grit ls-files`**
  - Recursive walk for active gitlinks (open submodule via `.git` gitfile, load its index, recurse).
  - Superproject-relative paths for matching and output; display paths from super work tree + submodule-through-cwd for `git -C subdir` (t3007 #15).
  - Skip glob expansion when `--recurse-submodules` (Git does not prune pathspec prefix).
  - `Pathspec::Glob` uses `pathspec_matches` so `?` matches `/`.
  - Fatal on `--recurse-submodules --error-unmatch`.
- **Harness**: `run-tests.sh t3007` → CSV/dashboards; `PLAN.md` marked complete.

## Verification

- `cargo fmt`, `cargo clippy --fix --allow-dirty`, `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t3007-ls-files-recurse-submodules.sh` → 24/24
