# t7426-submodule-get-default-remote

## Goal

Make `tests/t7426-submodule-get-default-remote.sh` pass (15/15).

## Changes

- **`grit/src/main.rs`**: Dispatch `submodule--helper` to `commands::submodule::run_submodule_helper`; register in `KNOWN_COMMANDS`.
- **`grit/src/commands/submodule.rs`**:
  - Implement `get-default-remote` with Git-compatible errors (`fatal: could not get a repository handle…`, usage exit 129).
  - Walk nested submodule paths via `.gitmodules` + gitlink segments.
  - Match submodule remote by resolved URL (path canonicalization) then fall back to default remote (`branch.*.remote`, single remote, else `origin`).
  - Fix relative submodule URL resolution: port Git `relative_url` / `get_up_path`; for repos under `.git/modules/<name>/`, read default remote from the **outer** superproject `.git` so nested `../innersub` resolves beside `sub/`, not under `super/`.
  - `submodule update`: path `.` selects all submodules; clone clears `GIT_DIR`/`GIT_WORK_TREE`; after clone set `remote.origin.url` to canonical clone source when possible.
  - `submodule sync`: super config URL vs submodule `remote.origin.url` (super vs sub_origin resolution).
- **`grit/src/commands/pull.rs`**: Detached HEAD + explicit remote with local path URL from `remote.<name>.url`; infer merge branch from remote HEAD.

## Verification

- `cargo fmt`, `cargo clippy --fix --allow-dirty`, `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7426-submodule-get-default-remote.sh` → 15/15 pass
