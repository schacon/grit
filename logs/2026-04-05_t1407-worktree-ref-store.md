# t1407-worktree-ref-store

- Date: 2026-04-05
- Task: fix `t1407-worktree-ref-store`
- Result: `4/4` upstream tests passing.

## Actions

- Read `AGENTS.md`, `PLAN.md`, and `git/t/t1407-worktree-ref-store.sh`.
- Reproduced the task with `CARGO_TARGET_DIR=/tmp/grit-build-t1407 bash scripts/run-upstream-tests.sh t1407 2>&1 | tail -40`.
- Added `grit test-tool ref-store` handling for worktree stores so the upstream helper can resolve refs and create symrefs against `worktree:main` and `worktree:<id>`.
- Implemented worktree-aware loose-ref lookup that checks the selected worktree admin dir before the common dir, which matches the linked-worktree layout created by Grit.
- Rebuilt `target/release/grit` and validated the worktree ref-store behavior in `/private/tmp` scratch repos.
- Fixed `scripts/run-upstream-tests.sh` so the generated fake `test-tool` wrapper preserves `"$1"` and `"$@"` instead of expanding them while the wrapper script is created.
- Re-ran `CARGO_TARGET_DIR=/tmp/grit-build-t1407 bash scripts/run-upstream-tests.sh t1407 2>&1 | tail -40` and confirmed `4/4` passing.
- Ran `cargo fmt`.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t1407 cargo test -p grit-lib --lib` and confirmed `96/96` passing.
- Attempted `CARGO_TARGET_DIR=/tmp/grit-build-t1407 cargo clippy --fix --allow-dirty`, but the sandbox blocked Cargo's lock manager with `failed to bind TCP listener to manage locking`.
- Updated `PLAN.md`, `progress.md`, and `test-results.md`.

## Notes

- `scripts/run-upstream-tests.sh` invokes `target/release/grit`, so rebuilding that binary was required before the upstream rerun reflected the Rust changes.
