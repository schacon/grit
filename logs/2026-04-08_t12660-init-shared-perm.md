# t12660-init-shared-perm

## Goal
Make `tests/t12660-init-shared-perm.sh` pass (37/37).

## Failure
Three stat-based checks expected `775` on `.git/objects` and `.git/refs`, and `664` on `.git/HEAD`. Grit left umask-only modes (`755`/`644` with umask 022).

## Fix
- Mirror Git’s `git_config_perm` / `calc_shared_perm` / `adjust_shared_perm` behavior in `grit/src/commands/init.rs`.
- Fresh init with no `--shared` and no `core.sharedRepository` in loaded config defaults to group-shared mode (`PERM_GROUP` = 0o660) and recursively chmods the new `.git` tree so directories become group-writable (775 under umask 022) and regular files like HEAD become 664—without writing `core.sharedRepository` (so tests that expect that key unset still pass).
- When `--shared` or config sets sharing explicitly, write `core.sharedRepository` and `[receive] denyNonFastforwards = true` like Git’s `init_db`.
- Reinit with no shared config uses `PERM_UMASK` (no chmod pass).

## Validation
- `./scripts/run-tests.sh t12660-init-shared-perm.sh` → 37/37
- `cargo clippy -p grit-rs --no-deps -- -D warnings`
- `cargo test -p grit-lib --lib`
