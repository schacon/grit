# t3423-rebase-reword

## Symptom

Harness showed 1/3: interactive todo lines with `reword` were dropped because `parse_todo_line_with_repo` only accepted pick/fixup/squash. First `pick` finished the rebase with one commit left on the todo. `FAKE_LINES="reword 2"` hit empty parsed todo → sequence editor failure.

## Fix (grit/src/commands/rebase.rs)

- Added `RebaseTodoCmd::Reword` and `reword`/`r` parsing in `parse_word`.
- `run_commit_editor_for_reword`: seed `COMMIT_EDITMSG`, `prepare-commit-msg` with source `reword`, run `GIT_EDITOR`, non-zero exit → "there was a problem with the editor".
- `cherry_pick_for_rebase`: restrict noop parent==HEAD fast path to `Pick` only; for `Reword` in that case still apply tree + editor + new commit. After merge, for `Reword` run editor instead of `prepare-commit-msg` with source `message`. On conflict for `Reword`, write `rebase-merge/message` (UTF-8 transcoded) plus `MERGE_MSG` via existing helper.
- `read_current_rebase_todo_cmd` + `do_continue`: handle `Reword` using `rebase-merge/message` as editor template when present.

## Validation

- `cd tests && GUST_BIN=.../grit ./t3423-rebase-reword.sh -v` → 3/3
- `./scripts/run-tests.sh t3423-rebase-reword.sh` → 3/3
- `cargo test -p grit-lib --lib`, `cargo fmt`, `cargo check -p grit-rs`
