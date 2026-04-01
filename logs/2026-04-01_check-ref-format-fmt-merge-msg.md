# check-ref-format and fmt-merge-msg — implementation log

## Date
2026-04-01

## Summary

Implemented two new plumbing commands: `grit check-ref-format` and
`grit fmt-merge-msg`.

## check-ref-format

**Library:** `grit-lib/src/check_ref_format.rs`

Implements `check_refname_format(refname, opts)` following git's
`check_refname_format()` in `git/refs.c` exactly:

- Rejects empty refs, lone `@`, leading `/` (without `--normalize`)
- Validates each component: no leading `.`, no `.lock` suffix
- Outer-loop checks: no `..`, no `@{`, no control chars/forbidden chars
- `--refspec-pattern`: allows one `*` wildcard
- `--allow-onelevel`: allows single-component names
- `--normalize`: collapses consecutive slashes and strips leading `/`,
  prints the result if valid

**CLI:** `grit/src/commands/check_ref_format.rs`

- `--branch` mode: validates the name is a legal branch name and prints it
  (rejects dash-prefixed names and `@{-N}` which require live repo state)
- Exit code 0 = valid, 1 = invalid (no output on error, matching git)

**Tests:** `tests/t1402-check-ref-format.sh` — 63 tests ported from
`git/t/t1402-check-ref-format.sh`, all passing.

## fmt-merge-msg

**Library:** `grit-lib/src/fmt_merge_msg.rs`

Parses FETCH_HEAD-style lines and produces a merge commit message:

- Skips `not-for-merge` lines
- Groups entries by source repository
- Handles `branch`, `tag`, `remote-tracking branch`, and generic kinds
- Suppresses `into main`/`into master` by default (git default behaviour)
- `--message`/`-m`: override title
- `--into-name`: override destination branch in `into <branch>`

**CLI:** `grit/src/commands/fmt_merge_msg.rs`

- `-m`/`--message`: custom first line
- `--into-name`: override target branch name
- `-F`/`--file`: read from file instead of stdin
- `--log[=N]`/`--no-log`: accepted for compatibility; log body not yet
  generated (title-only for now)

**Tests:** `tests/t5524-fmt-merge-msg.sh` — 11 tests, all passing.

## Test results

- `cargo test --workspace`: 33 unit tests pass (18 new)
- Harness: all 73 test suites, 0 failures
- New tests: t1402 (63 pass), t5524 (11 pass)
