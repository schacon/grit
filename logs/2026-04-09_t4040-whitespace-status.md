# t4040-whitespace-status

## Failures

- `git diff-tree -b` rejected `-b` (unknown option).
- `git diff-files -b` accepted `-b` but did not ignore whitespace-only index↔worktree changes, so `--exit-code` still exited 1 and `-p` still printed a hunk.

## Fix

- `grit-lib`: `normalize_ignore_space_change_line` + `normalize_ignore_space_change` for shared `-b` line normalisation.
- `diff-tree`: parse `-w/-b/--ignore-space-at-eol/--ignore-blank-lines`; filter modified blob pairs whose normalised content matches (after pathspec/pickaxe/`--find-object`).
- `diff-files`: track the same flags; skip bogus `M` when stat-untrusted but ws-normalised content matches index; post-filter entries; suppress patch hunks when ws-equivalent; use normalised text for stat/numstat when flags active.

## Verification

- `./scripts/run-tests.sh t4040-whitespace-status.sh` → 11/11
- `cargo test -p grit-lib --lib`
