# t5812-proto-disable-http

## Outcome

All 29 harness tests pass (`./scripts/run-tests.sh t5812-proto-disable-http.sh`).

## Root cause

1. **test-httpd** did not implement Apache `RewriteRule` redirects used by this file (`/ftp-redir/`, `/loop-redir/`, `/smart-redir-*`, `/dumb-redir/`). Without `/ftp-redir/` → `ftp://…`, Git never followed a redirect and the `GIT_ALLOW_PROTOCOL` + libcurl FTP check did not run.

2. **`test_grep` shadowing**: `tests/test-lib.sh` defined a simplified `test_grep` that stripped unknown flags (e.g. `-E`). The t5812 assertion uses `test_grep -E "(ftp.*disabled|…)"`; with `-E` dropped, `|` was not alternation and the grep failed. Removed the duplicate so `test-lib-functions.sh`’s upstream-style `test_grep` is used.

## Changes

- `grit/src/bin/test_httpd.rs`: `redirect_target` + `send_redirect` for the routes above.
- `tests/test-lib.sh`: remove duplicate `test_grep`.
- `PLAN.md` / `progress.md`, harness CSV + dashboards refreshed by `run-tests.sh`.
