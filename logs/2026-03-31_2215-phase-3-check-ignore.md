# Phase 3.1/3.2/3.3 — check-ignore subset

## Scope

- Implement `grit check-ignore` for a robust initial subset from `t0008`.
- Cover path arguments and stdin-based querying.
- Support `-v` / `--verbose`, `-n` / `--non-matching`, `--stdin`, `-z`, and `--no-index`.
- Implement ignore precedence for:
  - per-directory `.gitignore`
  - `.git/info/exclude`
  - `core.excludesfile`
- Port a coherent subset of upstream `git/t/t0008-ignores.sh` into `tests/t0008-ignores.sh`.

## Upstream/material reviewed

- `AGENT.md`
- `plan.md`
- `git/builtin/check-ignore.c`
- `git/Documentation/git-check-ignore.adoc`
- `git/t/t0008-ignores.sh`

## Implementation notes

- Added `grit-lib/src/ignore.rs`:
  - rule parsing for ignore files (comments, negation, directory-only, anchored patterns)
  - matching logic with last-match-wins precedence
  - repository-relative path normalization for query inputs
  - per-directory `.gitignore` loading/caching by directory chain
  - optional index integration (tracked files bypass ignore unless `--no-index`)
- Exported ignore module from `grit-lib/src/lib.rs`.
- Replaced `grit/src/commands/check_ignore.rs` stub with functional command:
  - manual argv parsing for supported options
  - option validation matching expected constraints
  - stdin LF and NUL modes
  - plain and verbose output modes (including NUL verbose format)
  - non-matching verbose records
  - exit status based on reportable pattern matches in selected subset

## Test port details

- Added `tests/t0008-ignores.sh` with 12 tests:
  - invalid invocation modes
  - basic path argument behavior and verbose output
  - tracked-vs-`--no-index` behavior
  - nested negation reporting
  - directory-only rule behavior
  - stdin (`--stdin`) and NUL (`-z`) modes
  - source precedence and output source attribution
- Added `t0008-ignores.sh` to `tests/harness/selected-tests.txt`.

## Validation

- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets -- -D warnings` -> PASS
- `cargo test --workspace` -> PASS (5 tests, 0 failures)
- `GUST_BIN=/Users/schacon/projects/grit/target/debug/grit TEST_VERBOSE=1 sh ./t0008-ignores.sh` -> PASS (12/12)

## Plan/progress/test-results updates

- `plan.md`: marked items `3.1`, `3.2`, and `3.3` as complete.
- `progress.md`: removed 3.x items from remaining and added completion notes.
- `test-results.md`: added Phase 3 check-ignore validation section and set latest update marker.
