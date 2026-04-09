# t7008-filter-branch-null-sha1

## Issue

`./scripts/run-tests.sh t7008-filter-branch-null-sha1.sh` reported `0/0` and `status=timeout` because the default per-file timeout was 30s while this file needs ~31s (filter-branch shells out heavily).

## Fix

Set `TIMEOUT=120` in `scripts/run-tests.sh` to match the script header comment (`default: 120`).

## Verification

`./scripts/run-tests.sh t7008-filter-branch-null-sha1.sh` → 6/6 pass.
