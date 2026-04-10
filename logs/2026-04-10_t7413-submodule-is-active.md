# t7413-submodule-is-active

## Goal

Make `tests/t7413-submodule-is-active.sh` pass (10/10).

## Changes

- Added `grit-lib/src/submodule_active.rs`: map worktree path → submodule name via `.gitmodules` (work tree or index blob); `is_submodule_active` matching Git `submodule.c` order (`submodule.<name>.active`, then `submodule.active` pathspecs with exclude handling, then URL fallback); `submodule_add_should_set_active` mirroring `submodule--helper.c` wildmatch on `submodule.active` patterns.
- `ConfigSet::has_key` in `config.rs` for “key present” without coercing bare keys to `"true"`.
- `test-tool submodule is-active` in `grit/src/main.rs`: exit 1 when inactive, print error and exit 0 when bare `submodule.active` (matches Git test expectations).
- `git submodule add`: only set `submodule.<name>.active` when `submodule_add_should_set_active` returns true.

## Validation

- `cargo build --release -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7413-submodule-is-active.sh` → 10/10

## Note

Full-workspace `cargo clippy -- -D warnings` still reports many pre-existing issues unrelated to this change.
