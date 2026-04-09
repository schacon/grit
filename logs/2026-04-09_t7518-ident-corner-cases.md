# t7518 ident corner cases

## Goal

Make `t7518-ident-corner-cases.sh` pass (5/5).

## Root cause

Harness exports `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` from `test-lib.sh`. Tests that `sane_unset` author name and use `git -c user.name=` expect Git’s `ident.c` behavior: an explicit empty `user.name` makes the default name empty (no `$USER` fallback), so commit fails with `empty ident name` and the role-specific hint.

Grit previously treated empty env as “unset” and fell back to `USER`, so `git -c user.name= commit` succeeded.

## Changes

- `grit-lib/src/ident_config.rs`: `ident_default_name` using `getpwuid_r` on Unix (matches Git passwd short name); empty `user.name` key yields `""`.
- `grit/src/ident.rs`: `GitIdentityNameEnv`, `read_git_identity_name_env`, `resolve_name` aligned with `fmt_ident` (resolve email first, empty name + crud checks, hints).
- Call sites: `commit_tree`, `revert`, `am`, `cherry_pick`, `format_patch` use env set/unset distinction and `ident_default_name`.
- `tests/t0110-environment.sh`: last case expected wrong behavior vs real Git; updated to `test_must_fail` + grep (now 31/31).

## Validation

- `sh tests/t7518-ident-corner-cases.sh -v`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7518-ident-corner-cases.sh t0110-environment.sh`
