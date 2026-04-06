# 2026-04-06 — t1090-sparse-checkout-scope

## Scope
- Target file: `tests/t1090-sparse-checkout-scope.sh`
- Starting status in plan: `2/7` passing (5 failing)
- Goal: make `t1090-sparse-checkout-scope` fully pass without modifying tests.

## Failures addressed
1. Sparse checkout scope was not consistently reapplied after checkout/merge/reset paths.
2. `checkout-index` did not accept `--ignore-skip-worktree-bits`.
3. `clone` did not accept `--template` compatibility path.
4. `config` global `-C` handling did not match expected no-op behavior for `git config -C <path>`.
5. `fetch` did not accept `--filter=blob:none` and needed sparse-aware blob retention.
6. `rev-list --missing=print` compatibility was incomplete for missing object reporting.

## Implemented fixes

### `grit/src/commands/checkout.rs`
- Reapplied sparse checkout behavior in branch/tree transitions so worktree materialization aligns with active sparse patterns.
- Ensured sparse pattern application occurs in reset/switch paths where required.

### `grit/src/commands/merge.rs`
- Reapplied sparse checkout bits after merge updates so skip-worktree state and working tree files stay consistent.

### `grit/src/commands/checkout_index.rs`
- Added `--ignore-skip-worktree-bits` option.
- When used, checkout now materializes those paths and clears skip-worktree state in index entries for the checked-out files.

### `grit/src/commands/clone.rs`
- Added `--template` option compatibility expected by the test flow.
- Adjusted template/info handling for empty-repo clone scenarios used by scope tests.

### `grit/src/commands/config.rs`
- Added support for global `-C`.
- Matched native behavior for `git config -C <path>` with no key/value (successful no-op).

### `grit/src/commands/fetch.rs` (+ call sites)
- Added `--filter` parsing (`blob:none` case used by tests).
- Implemented sparse-aware blob filtering semantics so required sparse-included blobs remain available.
- Updated fetch callers:
  - `grit/src/commands/pull.rs`
  - `grit/src/commands/remote.rs`

### `grit/src/commands/rev_list.rs` and `grit-lib/src/rev_list.rs`
- Added missing-object collection/propagation support for `--missing=print`.
- Improved output compatibility for missing-object reporting paths used by sparse+partial clone checks.

### `grit-lib/src/rev_parse.rs`
- Added `FETCH_HEAD` resolution support used by test flow.

### Supporting sparse path matching helpers
- Updated sparse pattern matching helpers and call sites (`checkout`, `fetch`, `sparse_checkout`, `ls_files`) to align with expected scope behavior and skip-worktree visibility.

## Validation
- `cargo fmt` ✅
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t1090-sparse-checkout-scope.sh` ✅ `7/7`
- `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated edits in non-target files)
- `cargo test -p grit-lib --lib` ✅ `96 passed`

## Tracking updates
- `PLAN.md`: marked `t1090-sparse-checkout-scope` complete (`7/7`).
- `progress.md`: updated counts to Completed `75`, Remaining `692`, Total `767`; recorded `t1090` in recent completions.
- `test-results.md`: appended latest validation/build/test evidence for this increment.
