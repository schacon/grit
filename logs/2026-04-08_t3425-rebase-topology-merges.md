# t3425-rebase-topology-merges

## Summary

Made `tests/t3425-rebase-topology-merges.sh` pass 13/13.

## Root causes

1. **`rebase --apply`** shells to `git format-patch` with `--cherry-pick --right-only --topo-order` on a symmetric range. Grit’s `format-patch` rejected unknown flags and did not implement that rev-list mode.
2. **Topological order** in `rev_list::topo_sort` used a max-heap on committer date, so `--reverse --topo-order` differed from Git (e.g. `e` before `n` instead of `n o e w` for `d...w`).
3. **Cherry-pick todo** for default rebase included merge commits; upstream’s patch series omits merges, so rebasing onto `c` must replay `d n o e` only and end at replayed `e`, not `w`.
4. **Merge replay**: flattening a merge whose second parent is the rebase onto must skip when `HEAD^{tree}` already matches the merge tree, or use a three-way with merge-base of the two parents (t3425 `e w` case).
5. **`git log A..B`**: log parsing treated `A..B` as tip-only; excluded side `A` was not applied, so `test_linear_range` saw too many commits.

## Files touched

- `grit-lib/src/rev_list.rs` — min-heap (`Reverse<CommitDateKey>`) for topo walk ready queue.
- `grit-lib/src/rev_parse.rs` — `try_parse_double_dot_log_range` for log-style two-dot ranges.
- `grit/src/commands/log.rs` — apply exclusions from `A..B`.
- `grit/src/commands/format_patch.rs` — compat flags + symmetric cherry path + skip merges in mbox output.
- `grit/src/commands/rebase.rs` — merge commit handling in cherry-pick; omit merges from todo unless `--rebase-merges`.

## Validation

- `cargo build --release -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t3425-rebase-topology-merges.sh` → 13/13
