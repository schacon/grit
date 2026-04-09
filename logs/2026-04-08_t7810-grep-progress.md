# t7810-grep progress (2026-04-08)

## Done

- Git-style pattern expression argv peeling (`-e`, `-f`, `--and`/`--or`/`--not`, `(`, `)`) before clap; `grep_expr` boolean eval with `--column` / `--invert-match` semantics aligned with git grep.c.
- `--only-matching` + `-w` uses git-style scan-from-`bol` / `cno` accumulation (matches column output for mmap tests).
- Pathspec-relative `--max-depth` (Git `within_depth`); glob pathspecs ignore max-depth; `.` pathspec matches all paths.
- `cwd_strip_repo_rel` for tree + worktree output (`HEAD:...` from subdir, unusual path quoting).
- Multiline first positional pattern splits into OR atoms; `-f -` reads stdin.
- Reject `\p{` / `\P{` in extended (non-Perl) mode for `grep.extendedRegexp=true` test.
- `log`: multiple `--author` / `--committer` / `--grep`; `--all-match` + `--invert-grep` on message grep; `--grep-reflog` for `-g`; `-F` applies to author/committer patterns.
- `reflog` wrapper updated for new `log::Args` fields.

## Harness (last run)

- `t7810-grep.sh`: 214 pass / 49 fail / 17 skip (263 total). Remaining gaps: `--no-index`, `-p`/`-W`, colorized context, FUNNYNAMES-dependent rows if prereq off, some combined log filter edge cases.
