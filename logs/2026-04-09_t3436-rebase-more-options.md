# t3436-rebase-more-options

## Summary

Made `tests/t3436-rebase-more-options.sh` pass (19/19) by extending `grit rebase` to match Git’s options and edge cases exercised by the suite.

## Changes (high level)

- CLI: `--ignore-whitespace`, `--committer-date-is-author-date`, `--reset-author-date`, `--ignore-date` (alias); argv preprocess `-r` / combined `r` → `--rebase-merges`.
- Persist flags under `.git/rebase-merge/` (`ignore-whitespace`, `cdate_is_adate`, `ignore_date`) and apply during replay and `--continue`.
- Merge backend: `ignore_space_change` in three-way merge; interactive `break`/`b`; `--continue` after break resumes `replay_remaining`.
- Identity: normalize author when cdate-only; committer matches author epoch `+0000` when both cdate and reset-author-date; clear `author_raw`/`committer_raw` when rewriting so `serialize_commit` does not prefer stale bytes; `commit_from_merged_index` preserves raw when options off.
- `--root` without `--onto`: ephemeral squash-onto commit (empty tree), with `+0000` idents when `--reset-author-date`.
- Preemptive FF: disabled for date options, interactive, and `rebase.rebaseMerges`; empty interactive todo still runs sequence editor.
- Cherry-pick filtering for interactive same as non-interactive; keep empty commits when `--reset-author-date` for second leg of t3436 test 18.
- After rebase finish: `reset_index_to_head` so next command does not see dirty index.
- Merge replay: rewrite HEAD after successful `merge -C` subprocess when replay options need new idents.
- `commit.rs`: `split_stored_author_line` is `pub(crate)` for rebase.
- `pull.rs`: struct update for new `Args` fields.

## Validation

- `./scripts/run-tests.sh t3436-rebase-more-options.sh`
- `cargo fmt`, `cargo clippy --fix --allow-dirty -p grit-rs`, `cargo test -p grit-lib --lib`
