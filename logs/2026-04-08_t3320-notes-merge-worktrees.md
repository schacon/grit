# t3320 notes merge worktrees

- Fixed `refs::common_dir` to honor absolute paths in `commondir` (matches Git / grit worktree add).
- Store `NOTES_MERGE_*` under each worktree’s gitdir (`repo.git_dir`); on conflict, scan main + `.git/worktrees/*` for another `NOTES_MERGE_REF` pointing at the same notes ref and error with Git’s `find_shared_symref` message.
- Skip common-dir fallback when resolving `NOTES_MERGE_REF` / `NOTES_MERGE_PARTIAL` so linked worktrees do not see the main repo’s merge state.
- Harness: `t3320-notes-merge-worktrees` 9/9.
