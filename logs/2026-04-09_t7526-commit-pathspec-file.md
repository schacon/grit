# t7526-commit-pathspec-file

## Goal

Make `tests/t7526-commit-pathspec-file.sh` pass against grit (commit `--pathspec-from-file`).

## Changes

- `grit/src/commands/commit.rs`: added `--pathspec-from-file` and `--pathspec-file-nul` (clap), read stdin (`-`) or file as raw bytes, parse via `crate::pathspec::parse_pathspecs_from_source` (same as `git add`).
- Validations aligned with upstream `git/builtin/commit.c`: `--pathspec-file-nul` requires `--pathspec-from-file`; conflicts with `--interactive`/`--patch`, `-a`, and trailing pathspec args; fatal messages prefixed with `fatal:` for harness `test_grep`.
- Empty pathspec with `--include` or `--only` (when Git would die in `prepare_index`): `No paths with --include/--only does not make sense.` — uses same `fixup` amend-style exception as Git (`fixup_prefix == "amend"`).

## Verification

- `cargo fmt`, `cargo build --release -p grit-rs`, `cargo clippy -p grit-rs -- -D warnings`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t7526-commit-pathspec-file.sh` → 11/11
