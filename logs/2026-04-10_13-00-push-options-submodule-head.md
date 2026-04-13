## Context

- Continued execution of the push implementation plan.
- Focused on closing the remaining `t5545-push-options.sh` failure (`push options and submodules`), while keeping previously passing push suites green.

## Root cause

- In the `push options and submodules` scenario, the submodule created by `submodule add` remained in detached HEAD state at the initial gitlink commit.
- Nested `push --recurse-submodules=on-demand` would push `HEAD` from that detached state, which can update a non-branch destination and leave `refs/heads/main` unchanged.
- The test asserts that submodule `main` on the remote advances, so this mismatch caused failure.

## Changes made

### `grit/src/commands/submodule.rs`

- Added `attach_submodule_head_to_default_branch(sub_git_dir, checked_out_oid)`.
- After `checkout_submodule_worktree`, attempt to reattach detached `HEAD` to the remote default branch (`refs/remotes/origin/HEAD`) when:
  - checkout left `HEAD` detached,
  - detached OID equals the checked-out OID,
  - the corresponding local branch can be created/resolved to that same OID.
- When reattachment succeeds, set tracking config:
  - `branch.<name>.remote = origin`
  - `branch.<name>.merge = refs/heads/<name>`
- This aligns `submodule add` behavior with Git in this push path and lets subsequent nested pushes naturally update the branch ref.

### `grit/src/commands/push.rs`

- Keep recursive push options resolved via effective options list (CLI or config fallback).
- For recursive submodule push refspec defaults, use `HEAD:<current-branch>` semantics and detached-head-safe rewriting for nested pushes so branch destinations are explicit.

### `grit-lib/src/push_submodules.rs`

- Relaxed `validate_submodule_push_refspecs` for `HEAD` source:
  - allow detached-HEAD submodules for `HEAD:<dst>` style pushes,
  - still allow symbolic HEAD when branch matches superproject branch.

## Validation

- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅ (166 passed)
- `./scripts/run-tests.sh t5528-push-default.sh` ✅ (31/32; 1 expected upstream `test_expect_failure`)
- `./scripts/run-tests.sh t5533-push-cas.sh` ✅ (23/23)
- `./scripts/run-tests.sh t5545-push-options.sh` ✅ (13/13)

## Notes

- Reverted unrelated formatter-only edits produced during lint/format passes to keep the commit scoped.
