# t0450-txt-doc-vs-help (2026-04-09)

## Problem

Harness reported 533/548: `-h` synopsis mismatches for `archive`, `range-diff`, `remote`, `rev-parse` (clap / hand-rolled usage vs adoc), and `submodule` (exit code / stream expectations vs t0450’s `help_to_synopsis`).

## Root cause

Upstream Git’s `git --list-cmds=builtins` is `list_builtins()` from `git.c` `commands[]` — it does **not** include `submodule` (submodule is a shell script). Grit incorrectly listed `submodule` in `builtins` / `parseopt`, so t0450 ran extra tests that assume exit **129** for every builtin `-h`, which does not match real `git submodule -h` (exit **0**).

## Fix

1. Added `grit/src/commands/upstream_help.rs` — shared adoc synopsis printing (`synopsis_for_builtin`, `write_upstream_synopsis`, `print_upstream_synopsis_and_exit`, `eprint_upstream_synopsis_and_exit`).
2. Wired `archive`, `range-diff`, `remote`, `rev-parse` to print upstream synopsis for lone `-h` / `--help` (129 vs 0).
3. Refactored `submodule` to reuse `upstream_help` for stderr usage and stdout `-h`.
4. Removed `submodule` from `print_list_cmds` `list-mainporcelain` and `parseopt` unions so `git --list-cmds=builtins` matches upstream.

## Verification

`./scripts/run-tests.sh t0450-txt-doc-vs-help.sh` → 542/542.

`cargo test -p grit-lib --lib` → pass.
