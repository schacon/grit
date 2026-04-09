# t6132-pathspec-exclude

## Goal

Make harness file `t6132-pathspec-exclude.sh` pass (31/31): Git exclude pathspecs (`:(exclude)`, `:!`, `:^`, `:/!`), all-negative lists, and commands run from a subdirectory.

## What changed (summary)

- **grit-lib `pathspec`:** `matches_pathspec_list` / `_with_context` / `_for_object` implementing Git’s positive-OR then exclude-OR subtraction; `extend_pathspec_list_implicit_cwd` for all-exclude lists without `:(top)`; `pathspecs_allow_bloom` ignores exclude specs; unit test for `:/!sub2`.
- **rev_list / log:** path touching uses list semantics; Bloom precheck skips exclude specs; `log` `--oneline` no longer overrides explicit `--format=%s` (`log_uses_builtin_oneline`).
- **pathspec (binary):** `resolve_pathspec` keeps long `:(…)` intact; cwd prefix for exclude short magic; `:/!` not stripped to literal `!…`.
- **Commands:** ls-files (magic + list match + `:/!` Pathspec), grep (`--untracked` walk, pathspec argv/cwd fixes), archive (cwd subtree), add (exclude-only staging, `-p` no longer no-op), clean (resolve magic + implicit cwd + dir context for `sub/`), reset (pathspec list → matched index paths), rm (list mode + stage 0), stash (resolve pathspecs), diff (trailing `--cached` in rev bucket), plus diff_files/diff_index/diff_tree/submodule using list helpers.
- **Harness:** `run-tests.sh t6132-pathspec-exclude.sh` → CSV + dashboards.

## Validation

- `./scripts/run-tests.sh t6132-pathspec-exclude.sh` → 31/31
- `cargo test -p grit-lib --lib`
