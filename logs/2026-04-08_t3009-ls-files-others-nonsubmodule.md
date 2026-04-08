# 2026-04-08 — t3009-ls-files-others-nonsubmodule

## Problem

`grit ls-files -o` listed `nonrepo-no-files/` (empty untracked directory). Upstream Git does not list empty dirs unless `--directory` is used.

## Fix

- `walk_worktree`: emit `name/` empty-directory markers only when `args.directory` is true (passed as `emit_empty_directories`).
- `dot_git_marks_git_repository`: treat only symlink `.git` or directory `.git` with `HEAD`/`commondir` as an embedded repo. A regular file `.git` is not a repo (t3000.6).

## Validation

- `./scripts/run-tests.sh t3009-ls-files-others-nonsubmodule.sh` → 2/2 pass.
- Manual check: plain `-o` omits empty dir; `-o --directory` matches Git.
