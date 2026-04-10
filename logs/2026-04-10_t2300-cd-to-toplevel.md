# t2300-cd-to-toplevel

## Goal

Make `tests/t2300-cd-to-toplevel.sh` pass (5/5).

## Findings

1. **Missing `git-sh-setup` on exec-path** — Tests prepend `$(git --exec-path)` to `PATH` and run `. git-sh-setup`. Grit’s exec path is the binary directory; only `grit` lived there. **Fix:** `grit/build.rs` copies `git/git-sh-i18n.sh` → `target/{profile}/git-sh-i18n` and `git/git-sh-setup.sh` → `git-sh-setup` with Makefile-style substitutions (`@PAGER_ENV@`, `@DIFF@`, strip `@BROKEN_PATH_FIX@`).

2. **`git-sh-i18n` fallbacks need `sh-i18n--envsubst`** — Sourced `git-sh-i18n` calls `git sh-i18n--envsubst`. **Fix:** new `grit/src/commands/sh_i18n_envsubst.rs` + dispatch `sh-i18n--envsubst` in `main.rs`.

3. **`rev-parse --git-dir` vs symlink cwd** — Rust `current_dir()` is often the real path while the shell uses a logical cwd via symlinks; relative paths like `../../.git` break `git-sh-setup`’s `git_dir_init`. **Fix:** when the relative git-dir would traverse `..`, print the absolute git dir (`rev_parse.rs`).

4. **Harness cwd vs preamble** — `test-lib-tap.sh` reset cwd to `$TRASH_DIRECTORY` *before* every `test_expect_success` body, undoing preamble `cd repo` before test 5 (`internal-link`). **Fix:** remove pre-test `cd` / `test_reset_cwd_to_trash` from TAP `test_expect_success` / `test_expect_failure`; keep post-test `cd` to trash. **Also:** drop pre-`eval` `cd` in `test-lib.sh` `test_eval_inner_` so preamble survives until the body runs.

## Validation

- `./scripts/run-tests.sh t2300-cd-to-toplevel.sh` → 5/5
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy -p grit-rs`
