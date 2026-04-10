# t3000-ls-files-others

## Goal

Make `tests/t3000-ls-files-others.sh` pass (15/15) — `git ls-files --others` with `--directory`, `--no-empty-directory`, pathspecs, and glob interaction.

## Changes (grit `ls_files.rs`)

1. **Trailing `/` on directory output** — `format_ls_display_path` / pathdiff dropped the slash; re-append when the repo-relative path ended with `/`.
2. **`--no-empty-directory` + short-circuit** — When `--directory` emitted a dir without recursing, files under that dir were never collected, so `path2/` vanished under `--no-empty-directory`. Pass `hide_empty_directories` into `walk_worktree` and recurse whenever it is set (Git always walks to classify emptiness).
3. **Pathspec-aware `--directory` walk** — Mirror Git `dir.c` `treat_directory`: emit `dir/` without recursing only when no pathspec requires deeper matches and the directory has no tracked descendants; glob pathspecs with wildcards force recurse under the literal prefix.
4. **`collapse_to_directories`** — Replaced first-segment-only collapse with grouping by top-level name: `path2/file2` + `path2-junk` → `path2/`; nested cases like `partially_tracked/untracked_dir/` + `untracked/deep/` stay distinct.

## Validation

- `cargo build --release -p grit-rs`
- `cargo check -p grit-rs`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t3000-ls-files-others.sh`

## Note

Workspace `cargo clippy -- -D warnings` fails on pre-existing grit-lib lints; used `cargo check` for this change set.
