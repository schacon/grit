# t5002-archive-attr-pattern

## Summary

Made `tests/t5002-archive-attr-pattern.sh` pass (19/19).

## Root causes

1. **Directory-only attribute patterns** — Rules like `ignored-only-if-dir/ export-ignore` must only match directories (Git `PATTERN_FLAG_MUSTBEDIR`). We stored the trailing `/` in the pattern string and matched basename-only, so `not-ignored-dir/ignored-only-if-dir` (a file) incorrectly matched. Parsed `must_be_dir` + `basename_only` on `AttrRule` and threaded `is_dir` through `get_file_attrs` / `path_has_gitattribute`.

2. **Empty parent directories in archives** — When an entire subtree is `export-ignore`d, Git omits parent directory entries. Added `prune_empty_directory_entries` after collecting archive entries.

3. **Tar directory mode 000** — Tree entries use mode `040000`; we wrote ustar headers with `mode & 0o7777` → `0`, producing `d---------` after extract so `test -e dir/file` failed. Use `tar_mode_for_git_mode` for directory headers and return `0755` for tree objects.

## Validation

- `bash tests/t5002-archive-attr-pattern.sh` (with `GUST_BIN` → grit)
- `./scripts/run-tests.sh t5002-archive-attr-pattern.sh`
- `cargo test -p grit-lib --lib`
- `cargo check -p grit-lib -p grit-rs`

## Note

`cargo clippy -- -D warnings` reports many pre-existing issues in the workspace; not run to completion.
