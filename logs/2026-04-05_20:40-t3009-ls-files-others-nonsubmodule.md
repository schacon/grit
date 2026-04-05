# t3009-ls-files-others-nonsubmodule

- Date: 2026-04-05 20:40 CEST
- Result: 2/2 upstream tests passing

## What changed

- No `ls-files` source fix was required in this turn: `target/release/grit` already passes upstream `t3009`.
- Updated the stale tracking entry in `PLAN.md` and recorded the completion in `progress.md`.
- Added the upstream verification summary to `test-results.md`.

## Verification

- Read `AGENTS.md`, the `t3009` entry in `PLAN.md`, and upstream `git/t/t3009-ls-files-others-nonsubmodule.sh`
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t3009 bash scripts/run-upstream-tests.sh t3009 2>&1 | tail -40`
- Confirmed `Tests: 2 (pass: 2, fail: 0)`
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t3009 cargo fmt --all 2>/dev/null; true`

## Notes

- The working tree already contained unrelated edits in `grit-lib/src/rev_parse.rs`, `grit/src/commands/rev_parse.rs`, and tracker/log files for other tasks; they were left untouched.
