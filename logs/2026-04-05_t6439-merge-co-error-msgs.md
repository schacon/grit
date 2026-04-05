## t6439-merge-co-error-msgs — 2026-04-05

### Goal
Make `tests/t6439-merge-co-error-msgs.sh` fully pass by aligning merge/checkout unpack-trees style diagnostics and merge preflight behavior.

### Initial state
- Harness: `./scripts/run-tests.sh t6439-merge-co-error-msgs.sh` → **1/6**
- Direct run showed failures in tests 2-6 initially, then narrowed to 2-3 after checkout message fixes.

### Root causes
1. **Fast-forward merge mutated HEAD/index/worktree before overwrite safety checks**
   - In `do_fast_forward`, `update_head()` ran before overwrite validation.
   - On expected failure (untracked overwrite), HEAD still advanced, causing follow-up scenario drift.

2. **Checkout diagnostics had duplicate `error:` prefix**
   - `checkout` constructed messages already beginning with `error:`, then top-level error reporter added `error: ` again.

3. **Merge overwrite diagnostics ordering/section formatting mismatched Git**
   - Mixed local+untracked case needed both sections in one error payload.
   - Second section requires explicit `error:` prefix when appended after first section.

4. **`GIT_MERGE_VERBOSITY=0` case needed strategy failure trailer**
   - Non-fast-forward preflight failure should append:
     `Merge with strategy ort failed.`
     after `Aborting`.

### Code changes

#### 1) Fast-forward preflight safety before mutation
- **File:** `grit/src/commands/merge.rs`
- **Change:** In `do_fast_forward`:
  - Compute `old_entries` and run `bail_if_merge_would_overwrite_local_changes(...)`
  - Only then call `update_head(...)`, `remove_deleted_files(...)`, `checkout_entries(...)`, and write index.

#### 2) Merge overwrite diagnostics harmonization
- **File:** `grit/src/commands/merge.rs`
- **Change:** Enhanced `bail_if_merge_would_overwrite_local_changes(...)`:
  - Detect both local-overwrite and untracked-overwrite sets in one pass.
  - Emit combined message in Git-compatible order:
    - local section
    - untracked section
    - `Aborting`
  - Use conditional untracked header prefix:
    - standalone untracked error: no inline `error:` (top-level prefix supplies it)
    - appended second section: explicit `error:` prefix.
  - Added `append_strategy_failed` flag to optionally append:
    - `Merge with strategy ort failed.`

#### 3) Strategy-verbosity trailer support
- **File:** `grit/src/commands/merge.rs`
- **Change:** `do_real_merge` now computes:
  - `append_strategy_failed = (GIT_MERGE_VERBOSITY == "0")`
  and passes it into overwrite preflight helper so trailer is appended at correct position.

#### 4) Checkout diagnostics prefix cleanup
- **File:** `grit/src/commands/checkout.rs`
- **Change:** Removed inline `error:` prefixes from constructed checkout refusal messages to avoid `error: error: ...` output.

### Regression interactions handled
- Kept `t6404` binary conflict line as stdout (`Cannot merge binary files: ...`) to preserve `t6404` expectations.
- Re-ran `t6404`, `t6415`, and `t6421` after t6439 changes to ensure no regressions.

### Validation
- Direct:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN=/workspace/target/release/grit bash tests/t6439-merge-co-error-msgs.sh` → **6/6**
- Harness:
  - `./scripts/run-tests.sh t6439-merge-co-error-msgs.sh` → **6/6**
- Merge regressions:
  - `./scripts/run-tests.sh t6404-recursive-merge.sh` → **6/6**
  - `./scripts/run-tests.sh t6415-merge-dir-to-symlink.sh` → **24/24**
  - `./scripts/run-tests.sh t6421-merge-partial-clone.sh` → **3/3**

### Outcome
- `t6439-merge-co-error-msgs` is now fully passing and marked complete.
