# t7425-submodule-gitdir-path-extension

## Goal

Make `tests/t7425-submodule-gitdir-path-extension.sh` pass (23/23).

## Changes (summary)

- **`submodule--helper`**: `main.rs` still dispatched to `submodule::run_submodule_helper`; wired `gitdir` / `migrate-gitdir-configs` to `commands::submodule_helper::run` and extended usage text.
- **Submodule gitdir layout**: `submodule add`, `clone --recurse-submodules`, `submodule update`, status gitfile refresh, and `submodule_separate_git_dir` resolve filesystem gitdirs via `submodule.<name>.gitdir` when `extensions.submodulePathConfig` is enabled; absorb in-tree `.git` into separate modules dir when adding an existing repo with the extension on.
- **URLs**: `resolve_submodule_super_url` treats `.` as the superproject work tree; `clone_submodules` uses it instead of joining relative URLs against `origin` only.
- **Clone**: `apply_default_submodule_path_config_from_global` mirrors `init.defaultSubmodulePathConfig`; `--jobs` forwarded to `submodule update`.
- **Push**: recursive object copy into nested `.git/modules/*`; before `updateInstead` worktree sync, copy loose/pack objects from each submodule git dir into the superproject ODB; use `checkout --force` for the remote work tree update (read-tree was too strict on gitlinks).
- **Nesting**: `path_inside_registered_submodule_name` + checks in `submodule add` when extension is off (reject `hippo/foobar` under submodule name `hippo`); always reject paths under another submodule’s registered **path**.
- **`git add` / gitlinks**: `stage_gitlink` resolves OID via `Repository::open(git_dir, Some(worktree))` + `resolve_head` so separate-git-dir submodules stage the checked-out commit.

## Validation

- `./scripts/run-tests.sh t7425-submodule-gitdir-path-extension.sh` → 23/23
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy -p grit-rs -p grit-lib --fix --allow-dirty` (workspace has many pre-existing clippy warnings)
