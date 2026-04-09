# t7810-grep progress (2026-04-09)

## Changes

- `pathspec.rs`: resolve `.` at repo root to `.` (not empty) so `--max-depth 0 -- . t` matches Git.
- `grep.rs`: avoid double-prefixing pathspecs after `pathspecs_relative_to_cwd` (fixes `grep -f` from subdir).
- `grep.rs`: binary-match messages use `path_for_output` on cwd-relative path (quoted names in subdir).
- `grep.rs`: `grep -L --cached` skips intent-to-add entries.
- `log.rs`: `--author` / `--committer` match ident with timestamp stripped after `>` (t7810 log grep, timestamp tests).

## Harness

After release build: **233/263** passing, **30** failing.

Remaining failures cluster around: `grep -p` / `-W` / `--max-count` with show-function, `--no-index` + discovery/fallback, parent-dir `git grep ..`, and full `color.grep.*` slot support.
