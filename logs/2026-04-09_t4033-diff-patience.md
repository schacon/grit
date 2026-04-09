# t4033-diff-patience

- Implemented diff algorithm + gitattributes loading for `grit diff`.
- Added `load_gitattributes_for_diff`, `count_changes_with_algorithm`, `matcher_for_path_parsed`.
- Wired algorithm + `diff --git`/`index` headers for `--no-index`.
- Harness: `./scripts/run-tests.sh t4033-diff-patience.sh` → 11/11.
- Commit: `7601b0f`. `git push origin` failed (remote not reachable in this environment).
