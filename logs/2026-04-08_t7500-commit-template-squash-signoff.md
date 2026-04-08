# t7500-commit-template-squash-signoff

- Extended `git commit` to match upstream t7500: `GIT_INDEX_FILE`, optional templates `:(optional)`, fixup/squash message assembly, amend/reword fixup tree semantics, editor + stripspace, signoff/template checks, status hints for unstaged-only commits, copy detection for ITA rename/copy via `diff_index_to_tree` + `status_apply_rename_copy_detection`.
- `git add` / `git ls-files`: honor `GIT_INDEX_FILE` for read/write.
- `git log` `%B`: full message with trailing newline (matches `get_commit_msg` / test expectations).
- Editor: `sh -c 'cmd "$1"'` for test `EDITOR` patterns; skip ineffective `:` placeholders; avoid launching `vi` when harness sets `EDITOR=:` / `VISUAL=:`.
- Harness: `./scripts/run-tests.sh t7500-commit-template-squash-signoff.sh` → 57/57.
