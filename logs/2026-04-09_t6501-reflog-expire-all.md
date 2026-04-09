## 2026-04-09 — t6501-freshen-objects (`reflog expire --expire=all`)

### Failure

Harness reported 39/42: all three failures were `disable reflogs` — `git reflog expire --expire=all --all` exited 1 with `invalid timestamp 'all' given to '--expire'`.

### Root cause

Git’s `parse_expiry_date` treats `all` and `now` as `TIME_MAX` for reflog expiry (every historical entry is older, so all are pruned). Grit parsed `all` as invalid and mapped `now` to the current wall time instead.

### Fix

`grit/src/commands/reflog.rs`: `parse_reflog_expire_cli` — return `i64::MAX` for `all` and `now` (case-insensitive); keep `0` → `now` for wall-clock “expire older than now” semantics used elsewhere.

### Verification

- `bash tests/t6501-freshen-objects.sh` (with `GUST_BIN`) → 42/42
- `./scripts/run-tests.sh t6501-freshen-objects.sh` → 42/42
- `cargo test -p grit-lib --lib` → pass
