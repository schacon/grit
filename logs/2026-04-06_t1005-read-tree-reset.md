# 2026-04-06 — t1005-read-tree-reset

## Scope
- Target file: `tests/t1005-read-tree-reset.sh`
- Starting status: `1/7` passing (`Not a directory (os error 20)` during `diff-files` dry-run checks)
- Goal: make `read-tree -u --reset` and porcelain equivalents (`reset --hard`, `checkout -f`) fully pass.

## Root causes
1. **Invalid D/F coexistence in index after `git add`**
   - Stage-0 entries could coexist for both `df` and `df/file`.
   - This caused `diff-files` to stat a child path under a tracked file (`df/file` under file `df`), yielding `ENOTDIR`.

2. **Worktree cleanup only considered stage-0 entries**
   - `read-tree`/`reset`/`checkout` worktree update helpers removed deleted paths using stage-0-only path sets.
   - Unmerged remnants (stage 1/2/3 paths like `old`) were not removed from disk in reset/checkout cleanup flows.

3. **`checkout -f` same-branch path bypassed tree-based cleanup**
   - Same-branch force checkout used `force_reset_to_tree()` from one code path and `force_reset_to_head()` from another.
   - One path failed to remove non-stage0 remnants consistently.

## Implemented fixes

### `grit-lib/src/index.rs`
- Hardened `Index::stage_file()` to resolve **both same-path conflict stages and D/F conflicts**:
  - removes exact path entries (all stages),
  - removes parent/child conflicting entries (`a` vs `a/b`).
- Added helpers:
  - `paths_conflict_for_df`
  - `path_is_parent_of`
- Added unit tests:
  - `stage_file_removes_same_path_conflict_stages`
  - `stage_file_removes_df_conflicting_entries`

### `grit/src/commands/read_tree.rs`
- Updated `checkout_index_entries()` deletion sets to use **all paths in index** (all stages), not only stage-0.
- Ensures `--reset -u` removes unmerged remnant working-tree files introduced by failed merges.

### `grit/src/commands/reset.rs`
- Updated hard/keep worktree updater (`checkout_index_to_worktree`) to compute removal sets from **all index paths**.
- Ensures `reset --hard` removes leftover files corresponding to unmerged entries absent from the target index.

### `grit/src/commands/checkout.rs`
- Updated checkout worktree updater to remove old paths using **all old/new index paths** (including non-stage0 entries).
- Simplified `force_reset_to_head()` to always delegate through `force_reset_to_tree()`, unifying force-reset semantics and cleanup behavior.

## Validation
- `cargo fmt` ✅
- `cargo build --release -p grit-rs` ✅
- `rm -rf /workspace/tests/trash.t1005-read-tree-reset && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t1005-read-tree-reset.sh` (from `/workspace/tests`) ✅ **7/7**
- `./scripts/run-tests.sh t1005-read-tree-reset.sh` ✅ **7/7**
- `cargo clippy --fix --allow-dirty && cargo test -p grit-lib --lib` ✅ (98/98)

## Tracking updates
- `PLAN.md`: marked `t1005-read-tree-reset` complete (`7/7`).
- `progress.md`: updated counts to Completed `81`, Remaining `686`, Total `767`; added `t1005` to recent completions.
- `test-results.md`: added build/test evidence for `t1005`.
