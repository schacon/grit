# t4015-diff-whitespace (2026-04-10)

## Result

Harness: **100 / 136** tests passing (`./scripts/run-tests.sh t4015-diff-whitespace.sh`).

## Implemented

- `update-index --chmod`: index-only (no worktree chmod); fixes mode-only `diff -w --exit-code`.
- `read_content_raw_or_worktree`: symlink via `read_link` (broken targets).
- Blob ↔ symlink: split into delete + add patches; `diff.color` / `color.diff` honored for `-c diff.color=always` with redirect.
- `diff --check`: hunk-based checker, `ParsedGitAttributes`, blank-at-EOF, conflicting `core.whitespace` → exit 128.
- `diff-index --check`, `diff-tree --check`: same checker + exit codes.
- `ws_check`: Git-style trailing whitespace; newline reconstruction for checkdiff.
- Colored `\ No newline`: BRED after `+` except symlink-add half of type-change split.

## Commit

`1a5c4ee` — push to `origin` failed (remote URL not reachable in this environment).

## Remaining failures (36)

CR/LF option diffs, `--ignore-blank-lines` hunks, rename/empty rename whitespace stats, `ws-error-highlight`, `--color-moved` suite, `--function-context` + ignore-blank-lines.
