## Task: t4044-diff-index-unique-abbrev

### Claim
- Claimed from `PLAN.md` after completing `t4064-diff-oidfind`.
- Baseline:
  - local mirror `./scripts/run-tests.sh t4044-diff-index-unique-abbrev.sh` → `0/2`
  - upstream harness `bash scripts/run-upstream-tests.sh t4044-diff-index-unique-abbrev` → `1/2`.

### Baseline failures
- Upstream failing assertion:
  - `git diff HEAD^..HEAD | grep index` expected unique short OIDs:
    - `index 51d27384..51d2738e 100644`
  - grit emitted fixed-width 7-char abbreviations, which are ambiguous in this case.
- Local mirror additionally fails setup due simplified helpers:
  - `test_oid`/`test_oid_cache` return `unknown-oid` placeholders, so expected tree OIDs do not match real object IDs.
  - This local mismatch is harness-specific and unrelated to `diff` abbreviation behavior.

### Root cause
- `grit diff` patch header writing used static abbreviation length (default 7 or CLI-provided),
  without unique-shortening against repository object namespace for `index <old>..<new>` lines.

### Implemented fix
- `grit/src/commands/diff.rs`
  - Threaded `Repository` into patch header writer path.
  - For `index` header OIDs (old/new), added dynamic abbreviation helper:
    - honors `--full-index` / explicit `--abbrev=<n>` / `--no-abbrev` behavior,
    - otherwise computes minimally unique abbreviations via
      `grit_lib::rev_parse::abbreviate_object_id(repo, oid, min_len)`.
  - Non-index contexts (raw output etc.) retain existing behavior.

### Validation
- `cargo build --release` ✅
- `bash scripts/run-upstream-tests.sh t4044-diff-index-unique-abbrev` ✅ `2/2`
- `./scripts/run-tests.sh t4044-diff-index-unique-abbrev.sh` ⚠️ `0/2` in this local mirror (known `test_oid` helper limitation; setup never reaches assertion in a meaningful way).
- Direct local script:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4044-diff-index-unique-abbrev.sh` ⚠️ fails in setup for same harness reasons.

### Outcome
- Upstream behavior for unique `index`-line abbreviations is complete (2/2 passing upstream).
- Keep task in-progress marker until this increment is included in the next coherent commit with updated tracking docs.
