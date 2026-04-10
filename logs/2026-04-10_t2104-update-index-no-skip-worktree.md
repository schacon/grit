## t2104-update-index-skip-worktree — 2026-04-10

- **Issue:** Tests 6–7 failed: `update-index --no-skip-worktree` did not clear skip-worktree because path processing hit the skip-worktree short-circuit (`continue`) before the flag handlers.
- **Fix:** In `grit/src/commands/update_index.rs`, run assume-unchanged and skip-worktree / no-skip-worktree handling before the “skip-worktree entries are not refreshed” block.
- **Verify:** `./scripts/run-tests.sh t2104-update-index-skip-worktree.sh` → 7/7; `cargo test -p grit-lib --lib` passed.
