## 2026-04-06 — t6418-merge-text-auto

### Scope
- Claimed `t6418-merge-text-auto` as active Rev Machinery target.

### Baseline reproduction
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6418-merge-text-auto.sh`
  - Baseline: **3/11 passing**.
- Initial failures:
  - Merge renormalization and EOL handling around `merge.renormalize` and `text=auto` transitions.
  - `checkout -m` path (treated as normal checkout with overwrite refusal instead of merge-style content application).
  - Modify/delete behavior in normalize/delete scenario.

### Root causes identified
1. `merge` ignored `merge.renormalize` config and `-Xrenormalize`/`-Xno-renormalize`.
2. Content merge paths compared raw ODB blobs only, so mixed-EOL histories produced clean merges where conflicts were expected (or vice versa).
3. Merge checkout path wrote worktree files with ODB bytes and bypassed worktree conversion in the normal index checkout helper.
4. `checkout -m` was parsed but never executed as branch-switch merge behavior.
5. `diff --no-index --ignore-cr-at-eol` was not honoring whitespace normalization in the no-index path, breaking tests that assert CRLF-insensitive comparisons.
6. Modify/delete handling considered mode-only differences as semantic content changes for this scenario.

### Implemented fixes
- `grit/src/commands/merge.rs`
  - Added merge renormalization control:
    - read `merge.renormalize` from config.
    - support `-Xrenormalize` and `-Xno-renormalize`.
    - threaded `merge_renormalize` through merge pipeline (`run` → octopus/single/virtual base → tree/content merge).
  - Added `renormalize_merge_blob(...)` helper:
    - virtual checkout + checkin (`convert_to_worktree` then `convert_to_git`) for three-way inputs.
  - Updated `try_content_merge` and `try_content_merge_add_add` to use renormalized buffers when enabled.
  - Updated clean merge writeback for stage-0 entries to run through `write_tree_entry_to_worktree(...)` so worktree conversion is applied consistently.
  - Refined modify/delete conflict classification:
    - treat same OID as unchanged regardless of mode-only drift for this merge content decision path.

- `grit/src/commands/checkout.rs`
  - Implemented branch-switch merge behavior for `checkout -m/--merge`:
    - route branch switch to `switch_branch(..., merge_mode=true)`.
    - route detached switch to `detach_head(..., merge_mode=true)`.
    - plumbed `merge_mode` through `switch_branch`, `detach_head`, `create_and_switch_branch`, `force_create_and_switch_branch`, and `switch_to_tree`.
    - in merge mode, force index/worktree update path instead of normal dirty-overwrite refusal semantics.

- `grit/src/commands/diff.rs`
  - Updated `run_no_index` to honor whitespace normalization flags:
    - build `WhitespaceMode`.
    - compare normalized text first and exit clean when equal.
    - generate patch output from normalized text when whitespace-ignore mode is active.
  - This fixed `git diff --no-index --ignore-cr-at-eol` behavior used by `t6418` checkout checks.

### Validation
- Direct:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t6418-merge-text-auto.sh` → **11/11**.
- Harness:
  - `./scripts/run-tests.sh t6418-merge-text-auto.sh` → **11/11** (final confirmation run).
  - Note: one transient harness re-run reported `7/11`, immediately followed by stable re-run at `11/11`.
- Regressions:
  - `./scripts/run-tests.sh t6433-merge-toplevel.sh` → **15/15**
  - `./scripts/run-tests.sh t6417-merge-ours-theirs.sh` → **7/7**
  - `./scripts/run-tests.sh t6400-merge-df.sh` → **7/7**
- Additional targeted verification:
  - `/workspace/target/release/grit diff --no-index --ignore-cr-at-eol` on LF vs CRLF equivalent files → exit `0` (expected).
