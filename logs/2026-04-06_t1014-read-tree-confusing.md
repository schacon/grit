# 2026-04-06 — t1014-read-tree-confusing

## Scope
- Target file: `tests/t1014-read-tree-confusing.sh`
- Start: `19/28` passing
- Current: `27/28` passing

## Root causes addressed
1. `read-tree` path validation only rejected exact `.git`/`git~1` patterns and missed several NTFS-protected confusing variants used by upstream tests.
2. Path validation did not reject backslash-containing names when NTFS protection was enabled.

## Implemented fixes

### `grit/src/commands/read_tree.rs`
- Extended path-component validation in `verify_path_component()`:
  - reject names containing `\` when `core.protectNTFS=true`.
  - reject NTFS-equivalent `.git` names with trailing dots/spaces and ADS-like forms (e.g. `.git...:stream`) using new helper `ntfs_equivalent_to_dotgit()`.
- Kept existing protections for:
  - `.`, `..`, exact `.git`,
  - case-insensitive `.git` checks under HFS/NTFS protections,
  - NTFS shortname `git~1`.

## Validation
- `cargo build --release -p grit-rs` ✅
- `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1014-read-tree-confusing.sh` ✅ `27/28` (only utf-8/HFS-off case remains failing under current harness env setup).
- `./scripts/run-tests.sh t1014-read-tree-confusing.sh` ✅ `27/28`.

## Remaining failure
- `FAIL 28: utf-8 paths allowed with core.protectHFS off`
  - Current harness invocation (`LC_ALL=C`) leaves `${u200c}` expansion effectively empty in this environment for this script, so the final test writes `.git` and is rejected by the always-on exact `.git` guard.
  - This is now isolated as the sole remaining failing case.

## Tracking updates
- `PLAN.md`: updated `t1014-read-tree-confusing` to `27/28`.
