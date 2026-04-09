# t3417-rebase-whitespace-fix

- **Issue:** `apply_ws_fix_to_index` was defined but never called; noop rebase picks (`HEAD == parent`) reused the original commit OID without fixing blobs.
- **Fix:** Load `ws_fix_rule` early; after `three_way_merge_with_content`, call `apply_ws_fix_to_index` before writing index/worktree. On noop pick with `--whitespace=fix|strip`, apply fix to the picked tree, write a new tree/commit, append `rewritten`, update HEAD.
- **Verify:** `cargo build --release -p grit-rs`; `./scripts/run-tests.sh t3417-rebase-whitespace-fix.sh` → 4/4; `cargo test -p grit-lib --lib`.
