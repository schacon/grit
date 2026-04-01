# Phase 6 diff-index (items 6.1, 6.2, 6.3, 6.4)

## Scope

- Command: `gust diff-index`
- Plan items: 6.1, 6.2, 6.3, 6.4
- Target behavior subset:
  - raw output
  - `--cached`
  - default (non-cached) mode for selected missing-file behavior
  - `-m` (match missing)
  - `--quiet` and `--exit-code`
  - `--abbrev[=<n>]`
  - basic pathspec filtering (`--` boundary and prefix/exact path matching)

## Implementation notes

- Replaced `gust/src/commands/diff_index.rs` stub with an implementation that:
  - resolves tree-ish to a tree object (commit peeled to tree),
  - loads and compares stage-0 index entries against tree entries (`--cached`),
  - in non-cached mode overlays worktree checks for missing/dirty tracked paths,
  - emits raw diff records in Git-like format,
  - supports exit-status behavior required by selected tests.
- Used existing `gust-lib` primitives (`Repository`, `Index`, object parsing, revision resolution, OID abbreviation) rather than introducing a separate diff engine crate for this subset.
- Fixed a pre-existing `rustfmt` blocker in `gust-lib/src/rev_list.rs` (let-chain syntax incompatible with current workspace edition/tooling).

## Test ports

- Added `tests/t4013-diff-various.sh` (focused `diff-index -m` subset).
- Added `tests/t4017-diff-retval.sh` (focused `diff-index` exit code/quiet/pathspec subset).
- Added `tests/t4044-diff-index-unique-abbrev.sh` (focused raw `--abbrev` width subset).
- Added these scripts to `tests/harness/selected-tests.txt`.

## Validation

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5/5)
- `GUST_BIN=/Users/schacon/projects/gust/target/debug/gust TRASH_DIRECTORY=... TEST_VERBOSE=1 sh ./t4013-diff-various.sh` -> PASS (4/4)
- `GUST_BIN=/Users/schacon/projects/gust/target/debug/gust TRASH_DIRECTORY=... TEST_VERBOSE=1 sh ./t4017-diff-retval.sh` -> PASS (5/5)
- `GUST_BIN=/Users/schacon/projects/gust/target/debug/gust TRASH_DIRECTORY=... TEST_VERBOSE=1 sh ./t4044-diff-index-unique-abbrev.sh` -> PASS (3/3)

## Remaining gaps (outside selected subset)

- `--patch` / `-p` is not implemented for `gust diff-index`.
- `--merge-base`, `--merge`, rename detection, and broader diffcore behaviors are not implemented in this increment.
- Pathspec support is basic (exact/prefix matching), not full magic-pathspec parity.
