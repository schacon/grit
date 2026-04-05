# t3302-notes-index-expensive — 2026-04-05

## Task
Fix the 1 remaining failing test in `t3302-notes-index-expensive.sh`.

## Investigation
- Read `/Users/schacon/projects/grit/AGENTS.md`, the `t3302-notes-index-expensive` entry in `/Users/schacon/projects/grit/PLAN.md`, and upstream `/Users/schacon/projects/grit/git/t/t3302-notes-index-expensive.sh`.
- Ran the requested command:
  `CARGO_TARGET_DIR=/tmp/grit-build-t3302 bash scripts/run-upstream-tests.sh t3302 2>&1 | tail -40`

## Findings
- The requested upstream harness command already reports `12/12` passing against `/Users/schacon/projects/grit/target/release/grit`.
- `data/file-results.tsv` already records `t3302-notes-index-expensive` as `12/12`.
- The remaining discrepancy was in the planning/documentation state, where `PLAN.md` still showed `11/12`.

## Changes
- Marked `t3302-notes-index-expensive` complete in `/Users/schacon/projects/grit/PLAN.md`.
- No Rust source changes were required because the current implementation already passes the upstream test file.
- Ran `cargo fmt` successfully.
- Attempted `CARGO_TARGET_DIR=/tmp/grit-build-t3302 cargo clippy --fix --allow-dirty`, but the sandbox blocked Cargo's TCP-based lock manager setup with `Operation not permitted (os error 1)`.
