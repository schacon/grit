# t3903-stash progress (2026-04-10)

## Summary

Improved stash/reflog/diff behavior; harness **101/142** passing (was 85/142 at start of session).

## Changes (high level)

- **Reflog**: `stash@{n}` resolves via `logs/refs/stash` (dwim `stash` → `refs/stash`); empty reflog message lines no longer written.
- **CLI**: Invalid `stash` options print `or:` lines to stderr; `pre_parse_stash_argv_guard` fixes `stash -q drop` vs `stash drop`.
- **Index lock**: stash create/push/apply/pop/branch preflight matches t3903 messages.
- **Stash tree**: merge HEAD paths missing from index when disk has content (post-`rm` + recreate); `reset_to_head` removes tracked paths not on HEAD; replace symlinks when writing regular files from HEAD.
- **Apply**: re-stage paths absent from current HEAD but present in stash index parent.
- **Show/list**: diff base is `stash^` (HEAD tree); Git-style index line + unified hunks; stat bar spacing; `--cc` list; `-N` list parsing.
- **Public**: `grit_lib::index::format_index_lock_blocked_detail`.

## Remaining failures (41)

Includes: stash branch with stash-like OID, bare-OID drop/pop errors, numeric stash refs, pathspec stash, export/import, skip-worktree, fsync batch, detached HEAD messages, submodule branch name in message, `stash list` edge cases vs full `git log`, etc.
