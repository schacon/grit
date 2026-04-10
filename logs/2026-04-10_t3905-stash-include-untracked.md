# t3905-stash-include-untracked

## Summary

Made `t3905-stash-include-untracked.sh` pass (34/34).

## Key fixes

- **Stash push `-u` / `-a`**: ignore rules via `IgnoreMatcher`, nested repo skip (reuse clean helpers), pathspec normalization, lib `pathspec_matches` for `:(glob)**/*.txt`, pathspec + untracked in `do_push_pathspec`, `matches_pathspec` uses `grit_lib::pathspec::pathspec_matches`.
- **`GIT_INDEX_FILE`**: `Repository::load_index()` now uses `index_path_for_env()` so `write-tree` / flows reading the default index see the alternate index (fixes synthetic stash commits in t3905.32).
- **Stash show**: merge global `-u`/`-p` from clap; duplicate check index vs untracked parent; resolve bare 40-char OIDs in `resolve_stash_ref`; stat/patch output aligned with Git (wide `-u` stat, index line for empty new files, stdout for “Saved…” messages).
- **Patch + `-u`**: exit 128 via `SilentNonZeroExit`; stderr message; pathspec-only untracked fallback from `-p`.
- **Porcelain v1**: removed forced `##` line on plain `--porcelain` (match Git / t3905.2).
- **Clean**: export `pathdiff` / `repo_relative_under_walk` for stash untracked walk; pathspec matching uses lib matcher.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t3905-stash-include-untracked.sh`
