# t1508-at-combinations

- Ran `./scripts/run-tests.sh --timeout 120 t1508-at-combinations.sh`: **35/35** pass (harness default 30s timeout can kill `-v` runs mid-script).
- **Rust fixes**
  - `git branch -u <local>`: `parse_upstream` now treats any resolvable `refs/heads/<name>` (including packed refs) as local tracking with `remote = .` (fixes `branch -u main`).
  - `commit` on a branch: append the same commit line to `logs/HEAD` as to the branch reflog (Git keeps both; fixes `HEAD@{n}`, `@{n}`, `@{now}` with `test_tick` timestamps).
  - `rev_parse::resolve_reflog_oid`: match Git’s `ref@{n}` indexing (`new_oid` from `entries[len-1-n]`, `@{0}` falls back to current ref when log empty, single-entry `@{1}` uses `old_oid`).
- Refreshed `data/test-files.csv` and dashboards from the harness run.
