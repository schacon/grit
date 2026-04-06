## 2026-04-06 — t6433-merge-toplevel

### Scope
- Claimed `t6433-merge-toplevel` as the active Rev Machinery target after pausing `t6409`.

### Initial actions
- Marked `t6433-merge-toplevel` in progress (`[~]`) in `PLAN.md`.
- Left `t6409-merge-subtree` tracked as partial (`7/12`) but not active.
- Next: reproduce current failures directly and via harness, then implement missing merge-top-level behavior.

### Baseline reproduction
- Direct: `GUST_BIN=/workspace/target/release/grit bash tests/t6433-merge-toplevel.sh`
  - Baseline: **7/15 passing**.
- Harness: `./scripts/run-tests.sh t6433-merge-toplevel.sh`
  - Baseline: **7/15 passing**.

### Root causes identified
1. `merge` treated `FETCH_HEAD` as a single revision token instead of expanding mergeable lines into multiple merge tips.
2. `merge` allowed multiple-head merge into unborn branch (`checkout --orphan ...`), but upstream rejects this.
3. Octopus fast-forward ancestry case still included current `HEAD` as merge parent; upstream drops redundant `HEAD` parent.
4. `--autostash` option parsing existed, but successful merge path did not restore dirty tracked files or emit `Applied autostash.` message expected by tests.

### Implemented fixes
- `grit/src/commands/merge.rs`:
  - Added `read_fetch_head_merge_oids(...)` and expanded `args.commits` when `merge FETCH_HEAD` is requested.
  - Added unborn guard in `merge_unborn(...)`: reject non-single-head merges with:
    - `Can merge only exactly one commit into empty head`.
  - In octopus merge commit construction:
    - detect `head_is_ancestor_of_all`.
    - when not `--no-ff`, omit `HEAD` from parent list in that ancestry-fast-forward case.
  - Added minimal tracked-file autostash helpers:
    - `capture_dirty_tracked_entries(...)`
    - `apply_autostash_entries(...)`
  - For `--autostash` in real merge path:
    - capture dirty tracked worktree entries,
    - skip overwrite refusal gate,
    - reapply snapshots after successful merge commit,
    - emit `Applied autostash.` to stderr.

### Validation
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6433-merge-toplevel.sh` → **15/15**.
- Harness:
  - `./scripts/run-tests.sh t6433-merge-toplevel.sh` → **15/15**.
- Regressions:
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **7/7**.
  - `./scripts/run-tests.sh t6439-merge-co-error-msgs.sh` → **6/6**.
  - Snapshot only (still partial): `./scripts/run-tests.sh t6409-merge-subtree.sh` → **7/12**.
