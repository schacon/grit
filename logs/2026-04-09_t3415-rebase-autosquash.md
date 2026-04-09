# t3415-rebase-autosquash (2026-04-09)

## Result

- Harness: **25/28** passing (`./scripts/run-tests.sh t3415-rebase-autosquash.sh`).
- Remaining failures: **abort last squash** (26), **fixup a fixup** (27).

## Rust changes (`grit/src/commands/rebase.rs`)

- Autosquash: `skip_fixupish_prefix` matches Git (`fixup! ` / `squash! ` / `amend! ` with space); command type uses `starts_with("fixup!")` / `amend!` vs squash (fixes `squash! squash!`).
- `is_final_fixup_in_todo`: hybrid — Git-style scan for trailing fixup/squash after skipping `noop`; when `rebase-merge/keep-empty` exists (`-k`), final after that scan; else also require `next_non_fixup_index` absent (preserves non-`-k` autosquash message folding).
- Squash/final fixup: run commit editor for `message-fixup` path too (Git `commit -e`); `run_commit_editor_for_template` + `run_commit_editor_for_reword` wrapper.
- Intermediate fixup: `message-fixup` + `prepare-commit-msg` + `rebase_commit_msg_cleanup` (t3415 `commit.cleanup` / hook appends).
- `replay_remaining`: on cherry-pick failure, keep current line in `todo` (`todo[i..]`); fix `end` to count non-empty lines when trimming todo after exec/global-exec failure.
- `do_skip`: when `interactive` marker present, drop first non-comment todo line before `replay_remaining`.
- `run_shell_editor`: inherit stdin/stdout/stderr for nested `sh -c` editor.
- `do_rebase`: validate `rebase.instructionFormat` when autosquash requested; `run_interactive_rebase` validates when autosquash rearranges inside `-i`.

## Follow-up (26–27)

- **26**: Final squash with failing `core.editor` must leave rebase stopped, allow amend + `rebase --skip` without completing the squash chain incorrectly.
- **27**: Folded message for chained squashes onto same empty tree should include **XZWY** (likely final-fixup / empty-merge / message accumulation ordering).
