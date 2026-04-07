# t3704-add-pathspec-file

## Goal

Make `tests/t3704-add-pathspec-file.sh` fully pass (11/11).

## Changes

- Added `pathspec::parse_pathspecs_from_source` in `grit/src/pathspec.rs`: NUL-separated mode (reject quoted lines like Git), line mode with CRLF strip, optional final line without newline, double-quoted C-style unquoting with octal escapes.
- `git add`: `--pathspec-file-nul`, read `-` via `read_to_end`, validate combinations (`--pathspec-from-file` vs interactive/patch/edit/extra pathspecs; `--pathspec-file-nul` requires `--pathspec-from-file`) before the interactive stub.
- Error string for pathspec+interactive/patch matches upstream grep: `'--interactive/--patch'` as one quoted token.
- `stage` command: pass `pathspec_file_nul: false` in `add::Args` literal.
- `test-tool parse-pathspec-file`: delegate parsing to shared helper.

## Verification

- `./scripts/run-tests.sh t3704-add-pathspec-file.sh` → 11/11
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo check -p grit-rs`, `cargo clippy -p grit-rs --fix --allow-dirty`

## Git

Branch: `cursor/t3704-pathspec-file-tests-3715`
