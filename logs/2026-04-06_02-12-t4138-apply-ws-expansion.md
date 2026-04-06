## Task: t4138-apply-ws-expansion

### Claim
- Claimed from `PLAN.md` after completing `t4102-apply-rename`.
- Baseline in local harness: `1/5` passing (`4` failures in whitespace-expansion apply cases).

### Baseline failures
- `apply with ws expansion (t=$t)` for test cases 1-4 failed with:
  - `context mismatch ... expected "\t..." got "                                                               ..."`
- Root cause: patch preimage lines use tabs, while target files use expanded spaces (`tabwidth=63`).
  Existing apply matching only tolerated direct equality or limited whitespace modes.

### Implementation
- Updated `grit/src/commands/apply.rs`:
  - Added compatibility `--apply` flag handling in previous task and kept behavior (info flags + apply).
  - Added `core.whitespace tabwidth=<N>` parsing into `ApplyWhitespaceMode`.
  - Introduced tab-expansion-aware normalization for context matching.
  - Generalized line matching to allow tab-expanded equivalence whenever:
    - `apply.whitespace=fix` (`ws_mode.whitespace_fix`), or
    - `--ignore-space-change` / `--ignore-whitespace` / `apply.ignorewhitespace=change`.
  - This allows hunks with tab-indented preimages to match files containing equivalent expanded spaces.
- Preserved existing behavior for:
  - `--inaccurate-eof`,
  - strict matching when no whitespace options/config are enabled.

### Validation
- `cargo build --release` ✅
- Local script:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4138-apply-ws-expansion.sh` ✅ `5/5`
- Tracked harness:
  - `./scripts/run-tests.sh t4138-apply-ws-expansion.sh` ✅ `5/5`
- Upstream harness:
  - `bash scripts/run-upstream-tests.sh t4138-apply-ws-expansion` ✅ `5/5`
- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (unrelated edits reverted)
  - `cargo test -p grit-lib --lib` ✅

### Status
- Task completed and ready to mark `[x]` in the plan.
