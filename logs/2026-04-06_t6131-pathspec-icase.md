## 2026-04-06 — t6131-pathspec-icase

### Scope
- Claiming `t6131-pathspec-icase` from plan as the next highest-priority remaining `t6*` item.

### Initial actions
- Marked `t6131-pathspec-icase` as in progress (`[~]`) in `PLAN.md`.
- Updated `progress.md` counts to keep completed/in-progress/remaining aligned with the plan.
- Next: reproduce failures directly and via harness, then implement pathspec `:(icase)` behavior gaps.

### Reproduction
- Direct run from `tests/` reproduced failures in prefix-aware `:(icase)` cases:
  - `tree_entry_interesting matches :(icase)bar with prefix`
  - `match_pathspec matches :(icase)bar with prefix`
  - `match_pathspec matches :(icase)bar with empty prefix`
- Observed over-broad matches included sibling directories (`FOO/BAR`, `foo/BAR`) when running from `fOo/`, indicating cwd-relative magic pathspec resolution was being folded into the icase-matched pattern.

### Root cause
- Magic `:(icase)` pathspecs were resolved by prepending `cwd_prefix` directly into the tail (e.g. `:(icase)fOo/bar`), then matching case-insensitively over the whole string.
- This made the cwd portion case-insensitive too, so sibling directories whose names differ only by case incorrectly matched.

### Fixes implemented
- `grit/src/pathspec.rs`
  - Extended magic parser to support an internal `prefix:` token in addition to `icase`.
  - Added `resolve_magic_pathspec(spec, cwd_prefix)` helper to produce resolved magic specs that preserve a case-sensitive cwd prefix while keeping `icase` behavior for the tail.
  - Updated `pathspec_matches` to enforce prefix filtering before tail matching for magic specs.
- `grit/src/commands/ls_files.rs`
  - Switched magic pathspec resolution in `resolve_pathspec` to use shared `resolve_magic_pathspec`.
  - Kept magic pathspecs as `Pathspec::Magic` and matched via shared matcher.
- `grit/src/commands/log.rs`
  - Added `resolve_effective_pathspecs` pass before revision walk.
  - Switched magic pathspec normalization to use shared `resolve_magic_pathspec` so `log -- ":(icase)..."` from subdirectories uses consistent semantics with `ls-files`.

### Validation
- `GUST_BIN=/workspace/target/release/grit bash tests/t6131-pathspec-icase.sh` → **9/9 pass**.
- `./scripts/run-tests.sh t6131-pathspec-icase.sh` → **9/9 pass**.
- Regressions:
  - `./scripts/run-tests.sh t6133-pathspec-rev-dwim.sh` → 6/6
  - `./scripts/run-tests.sh t6134-pathspec-in-submodule.sh` → 3/3
  - `./scripts/run-tests.sh t6136-pathspec-in-bare.sh` → 3/3
  - `./scripts/run-tests.sh t3004-ls-files-basic.sh` → 6/6
