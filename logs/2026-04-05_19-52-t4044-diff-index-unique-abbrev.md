## Task: t4044-diff-index-unique-abbrev

### Claim
- Claimed after completing `t4217-log-limit`.
- Marked as `[~]` in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t4044-diff-index-unique-abbrev.sh` reports `0/2` passing (`2` left).

### Investigation update
- Reproduced with direct run:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4044-diff-index-unique-abbrev.sh`
- Observed failures are currently blocked by local test harness limitations, not by `grit diff-index` abbreviation logic:
  - `test_oid_cache`/`test_oid` in `tests/test-lib.sh` are simplified and return placeholder `unknown-oid`, so setup assertion for deterministic fixture OIDs fails before abbreviation checks can be trusted.
  - Follow-on failure (`HEAD^` object not found) is a cascade from the setup mismatch.
- This task should be resumed once `test_oid` helpers in the harness align with the upstream expectations for this test file.
