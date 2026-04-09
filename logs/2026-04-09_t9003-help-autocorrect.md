# t9003-help-autocorrect

## Goal

Make `tests/t9003-help-autocorrect.sh` pass (10/10).

## Changes

1. **`grit/src/main.rs`** — Replaced ad-hoc unknown-command handling with Git-aligned `help_unknown_cmd` behavior:
   - Weighted Damerau–Levenshtein matching Git’s `levenshtein(..., 0, 2, 1, 3) + 1` and `SIMILARITY_FLOOR` (7).
   - Candidate set: builtins, `alias.*` names, and `git-*` executables on `PATH` (excluding exec-path duplicate scan).
   - Prefix boost for “common” porcelain commands (subset of `git/command-list.txt` mainporcelain + init/worktree/info/history/remote).
   - `help.autocorrect` parsing: `never`, `show`, `immediate`, bool, numeric delay; `prompt` gated on TTY stdin+stderr.
   - Messages and exit code 1 without extra `error:` line when listing suggestions.
   - Autocorrect reruns `alias::run_command_with_aliases` so corrected names resolve to aliases and `git-*` on PATH.

2. **`grit/src/alias.rs`** — `git-<cmd>` lookup: exec-path first, then `PATH` (matches Git’s dashed external resolution). Shared helper `find_git_external_helper`.

3. **`grit/src/commands/worktree.rs`** — Write per-worktree admin `config` with `core.worktree` when adding linked worktrees (and unborn `--orphan` path) so discovery does not treat worktrees from bare parents as bare when shared config has `core.bare = true`.

## Verification

- `./scripts/run-tests.sh t9003-help-autocorrect.sh` → 10/10
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`
