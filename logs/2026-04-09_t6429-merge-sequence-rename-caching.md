## 2026-04-09 — t6429-merge-sequence-rename-caching

- **Issue:** `t6429` failed test 5 (replay should stop on dir-rename conflict) and test 9 (`caching renames only on upstream side, part 2`) due to wrong tree after replay.
- **Root cause:** `replay_commits_onto` pre-applies rename detection + cached upstream renames, then called `merge_trees` with `MergeDirectoryRenamesMode::FromConfig`. The inner merge-ort directory-rename pass ran again and could relocate paths such as a recreated `olddir/unrelated-file` into `newdir/`, and could miss conflicts Git expects when upstream vs topic disagree on directory renames.
- **Fix:** Use `MergeDirectoryRenamesMode::Disabled` for `merge_trees_for_replay` so directory rename semantics come only from the replay-layer rename cache / `apply_directory_renames_to_ours_additions`, matching sequencer-style replay expectations in t6429.
- **Validation:** `./scripts/run-tests.sh t6429-merge-sequence-rename-caching.sh` → **11/11**; `cargo test -p grit-lib --lib` passed.
