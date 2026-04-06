## 2026-04-05 — t6414-merge-rename-nocruft

### Claim
- Marked `t6414-merge-rename-nocruft` as in progress in `PLAN.md`.

### Reproduction
- `./scripts/run-tests.sh t6414-merge-rename-nocruft.sh` → 2/3.
- Direct run confirmed failing test:
  - `merge blue into white (A->B, mod A, A untracked)`.

### Root cause
- In `grit/src/commands/merge.rs` (`merge_trees()`), case-1 rename handling
  treated `theirs.get(base_path)` as add-source too broadly.
- This could stage a tracked file at `base_path` even when theirs did **not**
  rename source away, causing checkout to overwrite untracked files at that path.

### Fix applied
- Restricted case-1 add-source insertion to only run when
  `theirs_renames.contains_key(base_path)`.
- Keeps true rename+add-source behavior while avoiding clobbering untracked files
  in the failing scenario.

### Next validation
- Re-run `tests/t6414-merge-rename-nocruft.sh`.
- Re-run `./scripts/run-tests.sh t6414-merge-rename-nocruft.sh`.
- Run nearby merge regression checks (`t6417`, `t6408`).

### Validation results
- `GUST_BIN=/workspace/target/release/grit bash tests/t6414-merge-rename-nocruft.sh` → **3/3 pass**.
- `./scripts/run-tests.sh t6414-merge-rename-nocruft.sh` → **3/3 pass**; TSV updated.
- Nearby regression checks:
  - `./scripts/run-tests.sh t6408-merge-up-to-date.sh` → **7/7 pass**.
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **6/7** (still failing; improved baseline tracked in TSV), no new regressions detected from this change.

### Tracking updates
- Marked `t6414-merge-rename-nocruft` complete in `PLAN.md` (3/3).
- Corrected stale `t6100-rev-list-in-order` entry to 3/3 complete (matches TSV).
- Updated `progress.md` counts from `PLAN.md`:
  - completed: 42
  - in progress: 0
  - remaining: 725
  - total: 767
