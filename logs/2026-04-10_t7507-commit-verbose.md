# t7507-commit-verbose

## Summary

Implemented Git-compatible `commit --verbose` / `commit.verbose` for the commit message template:

- Preprocess `commit` argv for `-v` / `--verbose` / `--no-verbose` ordering and strip `--no-verbose` before clap (`preprocess_commit_for_parse` + `GIT_GRIT_INTERNAL_COMMIT_VERBOSE`).
- Append scissors line + unified diffs via host `git diff` (raw patch, not comment-prefixed); `--cached` base is `HEAD^` when amending (root amend uses empty tree OID).
- `cleanup_message`-style path: truncate at scissors when verbose or `cleanup=scissors`; strip comments per `commit.cleanup` and full `core.commentChar` / `core.commentString`.
- `commit-msg` hook re-read applies the same cleanup for UTF-8 messages.
- `history` reword uses `comment_line_prefix_full` for cleanup.

## Validation

- `./scripts/run-tests.sh --timeout 120 t7507-commit-verbose.sh` — 45/45 pass.
- `cargo test -p grit-lib --lib` — pass.
