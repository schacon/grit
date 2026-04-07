# t6137-pathspec-wildcards-literal

## Goal

Make `tests/t6137-pathspec-wildcards-literal.sh` pass (25 tests): `git add` / `git commit` with shell-style pathspecs, escaped literals (`\*`, `\?`, `\[abc\]`), and wildcards.

## Changes

- **`grit/src/pathspec.rs`**: Added Git `simple_length` (treat `* ? [ \` as glob-special). `has_glob_chars` uses it. `pathspec_matches` compares literal prefix then `wildmatch` on the tail. Moved `resolve_pathspec` and `pathdiff` here for reuse.
- **`grit/src/commands/add.rs`**: Use shared pathspec helpers; `expand_glob_pathspec` uses `wildmatch`; skip `.git` when expanding; if pattern contains `[` and a file exists whose name equals the pattern string, include it (matches test expectation for `[abc]` + `a`).
- **`grit-lib/src/write_tree.rs`**: `write_tree_partial_from_index` merges `HEAD^{tree}` with index updates only for listed paths; `write_tree_from_index_subset` for root commit with pathspec.
- **`grit/src/commands/commit.rs`**: Pathspec staging returns matched path set; tree OID from partial merge when `HEAD` exists, else subset of index; same bracket + `.git` rules as add.

## Validation

- `./scripts/run-tests.sh t6137-pathspec-wildcards-literal.sh` — 25/25 pass
- `cargo test -p grit-lib --lib` — pass
- `cargo fmt`

## Note

System `git` 2.43 in this environment stages only `[abc]` for `git add "[abc]"`; the upstream test expects both `[abc]` and `a`. Grit follows the vendored test file in `tests/`.
