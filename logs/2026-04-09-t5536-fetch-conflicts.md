# t5536-fetch-conflicts

## Symptom

Harness showed 6/7: test "fetch conflict: arg vs. arg" failed. Manual run showed exit 1 with `error: glob refspec in upload-pack fetch not supported` instead of Git’s `fatal: Cannot fetch both …`.

## Cause

`git fetch` with CLI refspecs uses upload-pack negotiation. `collect_wants` in `fetch_transport.rs` bailed on any `*` in the source pattern, so negotiation failed before `fetch.rs` could run its destination conflict pre-check.

## Fix

- Added `match_glob_star_pattern` in `fetch_transport.rs` (single-`*` prefix/suffix match, same idea as `fetch.rs`).
- `collect_wants`: expand glob sources against the advertised ref list, dedupe OIDs via `push_want_unique`.
- Mirrored the same expansion in `http_smart.rs` `collect_wants_from_advertised` for HTTP fetch parity.

## Verification

- `bash tests/t5536-fetch-conflicts.sh` — 7/7 pass
- `./scripts/run-tests.sh t5536-fetch-conflicts.sh`
