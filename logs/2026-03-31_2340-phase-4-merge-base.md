# Phase 4 — merge-base (items 4.1, 4.2, 4.3)

## Scope

- Implement `grit merge-base` for:
  - default merge-base
  - `--all`
  - `--octopus`
  - `--independent`
  - `--is-ancestor`
- Cover corner cases in scope: disjoint histories, root ancestry graphs, repeated commits.
- Port and pass a coherent subset of `t6010-merge-base.sh`.

## References reviewed

- `AGENT.md`
- `plan.md`
- `git/builtin/merge-base.c`
- `git/commit-reach.c` (targeted function references)
- `git/Documentation/git-merge-base.adoc`
- `git/t/t6010-merge-base.sh`

## Implementation details

- Added `grit-lib/src/merge_base.rs`:
  - commit-spec resolution to commit IDs
  - ancestor-closure traversal with cached parent parsing
  - best-base reduction (remove ancestors of other candidates)
  - operations for default mode, octopus mode, independent reduction, and ancestry checks
- Exported module via `grit-lib/src/lib.rs`.
- Replaced `grit/src/commands/merge_base.rs` stub with CLI mode parsing and dispatch to library operations.
- Added `tests/t6010-merge-base.sh` (ported subset) with:
  - base graph setup
  - default and `--all` checks
  - `--independent` checks
  - `--is-ancestor` status checks
  - octopus-vs-default distinction
  - disjoint history and repeated-commit corner cases
- Added `t6010-merge-base.sh` to `tests/harness/selected-tests.txt`.

## Incidental fixes needed for required validation

- Fixed pre-existing compile/lint issues surfaced by workspace gates:
  - `grit-lib/src/ignore.rs`
  - `grit/src/commands/check_ignore.rs`

These were necessary to make `cargo clippy --workspace --all-targets -- -D warnings` succeed.

## Validation

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS
- `GUST_BIN=/Users/schacon/projects/grit/target/debug/grit TEST_VERBOSE=1 sh tests/t6010-merge-base.sh` -> PASS (`10/10`)

## Plan/progress/test updates

- Marked `plan.md` items `4.1`, `4.2`, `4.3` as complete.
- Updated `progress.md` counts and completed-task notes.
- Updated `test-results.md` with Phase 4 validation summary.
