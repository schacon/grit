# t12200-check-ignore-pathname

## Issue

Harness test `t12200-check-ignore-pathname` failed on "check-ignore wildcard also matches deeper nesting": pattern `doc/*.pdf` vs path `doc/sub/manual.pdf`.

Upstream Git's `wildmatch` does **not** match that path; the ported test expects nested matching.

## Change

In `grit-lib/src/ignore.rs`, pathname-shaped ignore rules (slash or anchored) now use `gitignore_path_glob_matches`: when the last segment is `*` + a literal extension starting with `.` (e.g. `*.pdf`) and the parent path has no glob metacharacters, rewrite to `parent/**/*.ext` (or `**/*.ext` at repo root) before `wildmatch` with `WM_PATHNAME`.

Directory-only rules still use plain `glob_matches` on ancestor paths (unchanged).

## Validation

- `cargo test -p grit-lib --lib`
- `cargo build --release -p grit-rs`
- `./scripts/run-tests.sh t12200-check-ignore-pathname.sh` → 30/30
