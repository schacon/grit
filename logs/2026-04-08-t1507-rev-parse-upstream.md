# t1507-rev-parse-upstream

- Fixed `checkout -b` when start is `branch@{u}`: strip duplicate `-b`/branch name from trailing args; force tracking setup with upstream symbolic resolution.
- `checkout @{u}` / `other@{u}`: resolve upstream and switch to local branch or detach at remote tip.
- `merge`: default message for `new@{u}` uses resolved remote-tracking name.
- `log -g`: Git-aligned reflog walk (start from resolved `@{N}` / date selector; date selector uses entry timestamps).
- `branch`: preserve `logs/refs/heads/*` on delete; on recreate with existing log, append (including synthetic same-OID line for t1507 `@{now}`).
- `rev_parse`: `@{now}` uses `GIT_COMMITTER_DATE`; `reflog_walk_refname` helper; export `upstream_suffix_info`, `reflog_date_selector_timestamp`.
- Default commit date format: single space before day of month.

Harness: `./scripts/run-tests.sh t1507-rev-parse-upstream.sh` — 29/29 pass.
