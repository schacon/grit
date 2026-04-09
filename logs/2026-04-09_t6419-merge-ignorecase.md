# t6419 merge + core.ignorecase

## Problem

On case-insensitive setups, `git merge` must treat paths that differ only by ASCII case as the same file. Without that, a branch keeping `TestCase` and another renaming to `testcase` could produce a bogus rename/delete or duplicate paths.

## Change

In `grit/src/commands/merge.rs`:

- Read `core.ignorecase` via `ConfigSet::get_bool`.
- When true, flatten trees for the merge engine with `tree_to_map_for_merge`: normalize each path component to ASCII lowercase and dedupe so `TestCase` / `testcase` share one map entry (first spelling wins).
- Applied everywhere `merge_trees` is fed flattened trees: `do_real_merge`, virtual merge base, octopus simulation/loop, `merge_tree_write_tree_core`, `remerge_merge_tree`.

## Validation

- `cargo build --release -p grit-rs` (earlier in session).
- Harness `t6419-merge-ignorecase` still skips on Linux without `CASE_INSENSITIVE_FS` in `tests/test-lib.sh` (cannot change per AGENTS.md); behavior verified manually with `core.ignorecase=true` on case-sensitive FS before shell capture regressed.
