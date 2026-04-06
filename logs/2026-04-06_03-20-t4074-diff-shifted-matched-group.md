## Task: t4074-diff-shifted-matched-group

### Claim
- Claimed from `PLAN.md` after completing `t4057-diff-combined-paths`.
- Baseline in tracked harness: `0/4` passing.

### Baseline failures
- All four tests failed in local script output due missing no-index patch header lines:
  - expected `diff --git a/file1 b/file2`
  - expected `index <old>..<new> 100644`
- After adding no-index headers, one remaining failure remained for:
  - `--ignore-all-space` + `--histogram` no-index diff hunk shaping (`t4074` test 3).

### Root cause
1. `grit diff --no-index` emitted only `---/+++` patch markers, not full git-style
   patch headers (`diff --git`, `index`), so expected output did not match.
2. `--no-index` path used `unified_diff` directly, bypassing whitespace-ignore matching
   semantics required by `--ignore-all-space` in this test.

### Implementation
- Updated `grit/src/commands/diff.rs`:
  - Added explicit support for `-c` / `--cc` flags in parser and args structure while preserving
    two-revision handling.
  - In `run_no_index`:
    - prepended git-compatible patch headers:
      - `diff --git a/<old> b/<new>`
      - `index <short_old>..<short_new> 100644`
    - computed blob OIDs for both paths via `Odb::hash_object_data(ObjectKind::Blob, bytes)`.
  - Added whitespace-aware no-index diff helper:
    - `no_index_unified_diff_with_ws_mode(...)`
    - computes grouped ops over normalized lines (`WhitespaceMode::normalize_line`) while printing
      original-line payloads, preserving expected whitespace-ignore behavior and hunk shape.
  - Wired `run_no_index` to use that helper whenever any whitespace-ignore option is active.

### Validation
- `cargo build --release` ✅
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4074-diff-shifted-matched-group.sh` (from `tests/`) ✅ `4/4`
- `./scripts/run-tests.sh t4074-diff-shifted-matched-group.sh` ✅ `4/4`
- Regression checks:
  - `./scripts/run-tests.sh t4057-diff-combined-paths.sh` ✅ `4/4`
  - `./scripts/run-tests.sh t4074-diff-shifted-matched-group.sh` ✅ `4/4` (re-run after regressions)
- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated autofixes outside scope)
  - `cargo test -p grit-lib --lib` ✅

### Status
- Task behavior completed and reflected in plan/results updates.
