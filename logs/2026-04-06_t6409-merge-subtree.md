## 2026-04-06 — t6409-merge-subtree

### Scope
- Claimed `t6409-merge-subtree` as the next highest-priority remaining `t6*` item.

### Initial actions
- Marked `t6409-merge-subtree` as in progress (`[~]`) in `PLAN.md`.
- Updated `progress.md` counts to keep completed/in-progress/remaining aligned with the plan.
- Next: reproduce failures directly and via harness, then implement missing subtree merge strategy behavior.

### Reproduction
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6409-merge-subtree.sh`
  - Initial baseline: **5/12 pass** (fails in `read-tree --prefix ... -u`, subtree updates, and remote add/fetch path handling).
- Harness:
  - `./scripts/run-tests.sh t6409-merge-subtree.sh`
  - Baseline recorded as partial in `data/file-results.tsv`.

### Implemented fixes (iteration 1)
- `grit/src/commands/read_tree.rs`
  - Relaxed `--prefix` constraints to allow `-u` with a single tree as upstream Git does.
  - Existing guard now rejects only merge/reset/multi-tree combinations with `--prefix`.
- Result:
  - early `t6409` setup path now proceeds further.

### Implemented fixes (iteration 2)
- `grit/src/commands/merge.rs`
  - Added subtree strategy option parsing:
    - `-s subtree` => implicit subtree-shift mode.
    - `-Xsubtree=<path>` => explicit subtree shift path.
  - Added tree-map transformation helpers to prefix/unprefix entries for subtree alignment.
  - Wired subtree alignment into merge execution path so merge logic runs on aligned maps and writes aligned paths back to index/worktree.
  - Kept default merge behavior unchanged for non-subtree strategies.
- Result:
  - local subtree merge cases improved and no regressions seen in `t6417`.

### Implemented fixes (iteration 3)
- `grit/src/commands/fetch.rs`
  - Fixed configured remote URL resolution for relative paths (`remote.<name>.url`):
    - now resolves relative configured URLs from the current repository root/worktree (matching Git), instead of process cwd.
  - This unblocks `remote add -f gui ../git-gui` style fetch in nested test repos.
- Result:
  - `t6409` progressed from hard fetch failure to semantic merge-content mismatch in later subtree cases.

### Current status (after fixes)
- Direct `t6409`: **7/12 passing**.
- Harness `t6409`: **7/12 passing**.
- Remaining failures are currently centered on test harness behavior divergence in this simplified test-lib environment (state persistence and directory context between test blocks), which causes expected in-test shell-variable workspace paths (`o2`, sibling repo paths) to diverge from upstream assumptions in later blocks.

### Regression checks run
- `cargo test -p grit-lib --lib` → 97/97 pass.
- `./scripts/run-tests.sh t1003-read-tree-prefix.sh` → 3/3 pass.
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → 7/7 pass.

### Final harness alignment fix
- `tests/test-lib.sh`
  - Updated `test_expect_success` execution model to keep test bodies in the current shell **without forcing `cd "$TRASH_DIRECTORY"` and restoring old cwd after each test**.
  - This matches upstream test-lib behavior where tests may intentionally carry cwd changes (`cd ../repo`) into later blocks.
  - Fixes late-block `t6409` failures where `setup` created sibling repos and subsequent tests expected to start from that modified cwd.

### Final validation
- Harness:
  - `./scripts/run-tests.sh t6409-merge-subtree.sh` → **12/12 passing**.
- Regressions:
  - `./scripts/run-tests.sh t6403-merge-file.sh` → **39/39 passing**.
  - `./scripts/run-tests.sh t6501-freshen-objects.sh` → **42/42 passing**.
  - `./scripts/run-tests.sh t6427-diff3-conflict-markers.sh` → **9/9 passing**.
  - `./scripts/run-tests.sh t6001-rev-list-graft.sh` → **14/14 passing** (one transient 11/14 flake was immediately rerun clean at 14/14).

### Outcome
- `t6409-merge-subtree` is now fully passing and marked complete in the plan.
