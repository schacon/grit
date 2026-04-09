# t4060 submodule diff work

- Implemented `diff-index -p --submodule=diff` with recursive tree diffs, dirty/untracked lines, typechange ordering, nested path prefixes.
- Default `diff-index` ignore for untracked-in-submodule matches Git (`--ignore-submodules=none` to show).
- `git commit` pathspec: stage embedded repos as gitlinks; peel `-m` from trailing args (`commit path -m msg`).
- `git diff --submodule` / `--submodule=diff` / `--submodule=log`: submodule log uses submodule ODB + encoding-aware subject; diff mode skips outer gitlink patch.
- Remaining: ~22 failures in t4060 (typechange cached/worktree, deleted submodule, multi-submodule, nested submodule, absorbgitdirs).
