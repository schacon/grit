# t3420-rebase-autostash (2026-04-09)

## Changes

- **checkout**: Stop deleting untracked files that would be overwritten by checkout; refuse like Git (t3420 checkout-onto failures).
- **stash**: `reset_to_head` removes paths tracked before reset but absent from HEAD so autostash can clear conflicting paths before checkout (t3420 conflicting stash + `--apply`).
- **stash show -p**: Diff old side = `stash^1` tree (HEAD at stash time); unified hunks via `unified_diff` with Git-style index line; global `-p` on `git stash show` enables patch mode.
- **rebase --quit**: `save_autostash` behavior — store autostash on `refs/stash`, print Git messages, remove `autostash` file.
- **rebase**: Preserve human upstream spec when `rebase <upstream> <branch>` (do not replace with hex before `do_rebase`); snapshot `preserve-onto-refs` for heads at onto before rebase and restore after finish if they incorrectly match new tip.
- **rebase output**: Autostash apply/conflict + merge success on stderr; stdout flush before stderr autostash lines; merge backend omits `First, rewinding` / `Applying:` (apply keeps them).

## Status

`./scripts/run-tests.sh t3420-rebase-autostash.sh` still reported **4 failing** tests (output checks 9/24/39 and test 53). Trash `actual` showed an extra `rebasing N commits onto <abbrev>` line and wrong autostash ordering vs expected; root cause not fully isolated in this session.

## Reason if stopping

`blocked` on matching exact merged stdout/stderr ordering and eliminating the stray progress line without a reliable source in-tree for that string.
