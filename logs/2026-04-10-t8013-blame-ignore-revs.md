# t8013-blame-ignore-revs

## Failure

`bad_files_and_revs` failed: `git blame file --ignore-rev NOREV` did not report `cannot find revision NOREV to ignore`.

## Root causes

1. Clap stopped parsing at the positional file when options followed the pathspec (Git allows trailing options). `--ignore-rev` was not applied; `NOREV` was parsed as a revision for `parse_blame_args`, producing the ambiguous-argument fatal.
2. `--ignore-rev` used `resolve_revision` with index DWIM, which could mis-handle unknown names compared to Git’s blame ignore path.

## Fixes

- `preprocess_blame_argv` in `grit/src/main.rs`: move a trailing block of known blame flags before pathspec tokens; insert `--` when needed so clap sees `[options] -- <path> [<rev>]`.
- `grit/src/commands/blame.rs`: resolve `--ignore-rev` and ignore-file lines with `resolve_revision_without_index_dwim`; map failures for CLI `--ignore-rev` to `cannot find revision … to ignore` (no chained context).
- Wired the same preprocessing for `annotate`.

## Verification

`./scripts/run-tests.sh t8013-blame-ignore-revs.sh` → 19/19 pass.
