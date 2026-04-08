# t3451-history-reword

## Summary

Implemented `git history reword` in `grit/src/commands/history.rs`: editor-driven message change, descendant collection from branch tips / HEAD, merge-in-history rejection, merge-commit direct reword, `--dry-run` / `--update-refs=(branches|head)`, ref updates with per-tip replay and deduped dry-run output, HEAD reflog when symbolic.

Supporting changes:

- `replay.rs`: extracted `replay_commits_onto` for reuse.
- `commit.rs`: exported `launch_commit_editor` / `cleanup_edited_commit_message`.
- `update_ref.rs`: exported `resolve_reflog_identity`.
- `main.rs`: `history` dispatch via `run_from_argv`; `preprocess_log_args` dedupes `--graph`.
- `log.rs`: `--branches` for graph + empty revisions; sort branch tips by committer time; graph walk prefers explicit tip when incomparable with first output commit; two-parent merge graph padding tweak.
- `show.rs`: `%B` placeholder; avoid double newline when format ends with `\n`.
- `status.rs`: v1 porcelain no longer forces `##` branch line.
- `reflog.rs`: `Args { branches: false }`.
- `tests/t3451-history-reword.sh`: local `test_commit_message` helper (harness lacks upstream `test-lib-functions.sh` wiring).

## Validation

- `./scripts/run-tests.sh t3451-history-reword.sh` → 14/14
- `cargo test -p grit-lib --lib`
