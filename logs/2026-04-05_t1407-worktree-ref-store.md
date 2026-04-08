# t1407-worktree-ref-store

- Date: 2026-04-05
- Task: fix `t1407-worktree-ref-store`
- Result: `4/4` upstream tests passing.

## Failure analysis

- Tests initially failed because `test-tool ref-store` was missing:
  - `error: test-tool: unknown subcommand 'ref-store'`

## Implementation

- Added `grit test-tool ref-store` handling for worktree stores so the upstream helper can resolve refs and create symrefs against `worktree:main` and `worktree:<id>`.
- Updated `grit/src/main.rs`:
  - `run_test_tool_ref_store()` and dispatch under `test-tool`;
  - worktree selector resolver for `worktree:<id>` (`main` and linked worktrees under `.git/worktrees/<id>`);
  - `resolve-ref <ref> <flags>` output shape expected by tests (shared refs vs per-worktree HEAD);
  - `create-symref <name> <target> <logmsg>` for worktree-specific symbolic refs.
- Implemented worktree-aware loose-ref lookup that checks the selected worktree admin dir before the common dir, which matches the linked-worktree layout created by Grit.
- Fixed `scripts/run-upstream-tests.sh` so the generated fake `test-tool` wrapper preserves `"$1"` and `"$@"` instead of expanding them while the wrapper script is created.

## Actions

- Read `AGENTS.md`, `PLAN.md`, and `git/t/t1407-worktree-ref-store.sh`.
- Reproduced the task with `CARGO_TARGET_DIR=/tmp/grit-build-t1407 bash scripts/run-upstream-tests.sh t1407 2>&1 | tail -40`.
- Rebuilt `target/release/grit` and validated the worktree ref-store behavior in `/private/tmp` scratch repos.
- Re-ran upstream harness and confirmed `4/4` passing.
- `./scripts/run-tests.sh t1407-worktree-ref-store.sh` and direct `GUST_BIN=... bash tests/t1407-worktree-ref-store.sh` — **4/4**.
- Ran `cargo fmt`.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t1407 cargo test -p grit-lib --lib` and confirmed `96/96` passing.
- Attempted `CARGO_TARGET_DIR=/tmp/grit-build-t1407 cargo clippy --fix --allow-dirty`, but the sandbox blocked Cargo's lock manager with `failed to bind TCP listener to manage locking`.
- Updated `PLAN.md`, `progress.md`, and `test-results.md`.

## Notes

- `scripts/run-upstream-tests.sh` invokes `target/release/grit`, so rebuilding that binary was required before the upstream rerun reflected the Rust changes.
