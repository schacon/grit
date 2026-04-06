## t1014-read-tree-confusing — 2026-04-06

### Goal
- Make `tests/t1014-read-tree-confusing.sh` fully pass.

### Root cause
- The upstream script expects `${u200c}` (zero-width non-joiner) to be available by default.
- Our lightweight test harness did not define `u200c`, so in many runs those path cases degraded into plain `.Git` variants and produced inconsistent behavior depending on locale/env.
- Separately, `read-tree` path protection needed to correctly reject HFS-confusable `.git` spellings containing zero-width Unicode marks while still allowing non-confusable UTF-8 names when `core.protectHFS=false`.

### Code changes

1) `tests/test-lib.sh`
- Added a default `u200c` definition:
  - `u200c=${u200c:-$(printf '\342\200\214')}`
  - `export u200c`
- This aligns the harness with upstream assumptions and stabilizes `${u200c}` expansion in test scripts.

2) `grit/src/commands/read_tree.rs`
- Added Unicode-format-character stripping for HFS/NTFS dotgit checks in path component validation.
- Introduced:
  - `is_unicode_format_char(ch: char) -> bool`
  - `dotgit_hfs_normalized(name: &[u8]) -> Option<String>`
- Updated `verify_path_component` to:
  - continue exact `.git` rejection,
  - continue case-insensitive `.git`/`git~1`/NTFS ADS/backslash checks,
  - additionally reject names that normalize to `.git` after removing Unicode format characters (e.g., zero-width non-joiner insertions), while preserving acceptance of non-dotgit UTF-8 names.

### Validation

- Direct with explicit zero-width variable:
  - `u200c=$'\u200c' GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t1014-read-tree-confusing.sh`
  - Result: **28/28 passing**

- Direct under harness locale/env (`LC_ALL=C` path):
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C _prereq_DEFAULT_REPO_FORMAT=set GUST_BIN=$(pwd)/grit bash t1014-read-tree-confusing.sh`
  - Result: **28/28 passing**

- Standard direct run:
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t1014-read-tree-confusing.sh`
  - Result: **28/28 passing**

- Harness update:
  - `./scripts/run-tests.sh t1014-read-tree-confusing.sh`
  - Result: **28/28 passing**

- Regressions:
  - `./scripts/run-tests.sh t1416-ref-transaction-hooks.sh` → **10/10**
  - `./scripts/run-tests.sh t1403-show-ref.sh` → **12/12**
  - `./scripts/run-tests.sh t1421-reflog-write.sh` → **10/10**

- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty -p grit-rs` ✅ (reverted unrelated edits)
  - `cargo test -p grit-lib --lib` ✅ (98/98)

