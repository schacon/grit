# t2030-unresolve-info (partial)

- Implemented Git index `REUC` (resolve-undo) in `grit-lib`: parse/write extension, `Index::stage_file` records unmerged stages, `remove`/`unmerge_path_from_resolve_undo`, `clear_resolve_undo`.
- `grit ls-files --resolve-undo` with full 40-char OIDs (matches test `rev-parse` + `test_cmp`).
- `grit update-index`: `--clear-resolve-undo`, real `--unresolve`, `--cacheinfo` / path updates use `stage_file` for stage 0 so REUC is recorded; `--again` uses `stage_file`.
- `grit checkout`: `unmerge_paths_in_index` consumes REUC (Git `unmerge_index`); `switch_to_tree` clears REUC; `switch_force` no longer treats `-m` as `-f` (fixes resolve-undo cleared on `checkout second^0`); `merge_branch_working_tree` no longer copies old REUC into merged index; extra `clear_resolve_undo` before index writes on forced paths; `force_reset_*` clears REUC before write.
- `grit reset --hard` (mixed/hard path): clear REUC when rebuilding index.
- `grit merge`: clear REUC on disk before merge (Git `resolve_undo_clear_index`).
- `grit fsck --unreachable`: seed walk from index entries + REUC OIDs; honor `GIT_INDEX_FILE`.
- `grit-lib rerere`: conflict scan matches Git `find_conflict` (three-way only); replay tries worktree then synthesized index conflict for `try_replay_merge`.

Harness: `./scripts/run-tests.sh t2030-unresolve-info.sh` → **9/14** pass (tests 10–14 still fail: rerere forget / MERGE_RR + final gc/fsck block).
