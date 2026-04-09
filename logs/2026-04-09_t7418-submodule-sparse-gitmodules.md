# t7418-submodule-sparse-gitmodules

## Summary

Made `tests/t7418-submodule-sparse-gitmodules.sh` pass 9/9.

## Changes

1. **`read-tree -u`**: Treat `160000` gitlink index entries like clone — ensure an empty directory only; do not read the gitlink OID as a blob in the superproject ODB (fixes sparse checkout after clone).

2. **`test-tool submodule`**: Implemented `config-list`, `config-set`, `config-unset`, `config-writeable` in `grit` (read `.gitmodules` from worktree, index, or `HEAD` tree; write guard matches Git `is_writing_gitmodules_ok`). Wired `tests/test-tool` to delegate `submodule` to grit (with `-C`).

3. **`submodule summary`**: Added `--for-status`; skip when `submodule.<name>.ignore=all` in `.gitmodules` or `config`; merge in gitlink paths where index matches `HEAD^{tree}` but submodule worktree HEAD differs (parity with `diff-index --ignore-submodules=dirty`).

4. **`submodule add`**: Fail early when writing `.gitmodules` is unsafe (same guard as Git).

5. **Tracking**: `PLAN.md`, `progress.md`, `test-results.md`, harness CSV/dashboards via `run-tests.sh`.

## Validation

- `cargo fmt`, `cargo clippy --fix --allow-dirty -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7418-submodule-sparse-gitmodules.sh`

## Git

Branch: `cursor/t7418-submodule-sparse-gitmodules-7843` — commit and push to origin.
