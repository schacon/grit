## t4208-log-magic-pathspec

### Claim + baseline

- Claimed `t4208-log-magic-pathspec` from the Diff section in the plan.
- Baseline before fixes:
  - `./scripts/run-tests.sh t4208-log-magic-pathspec.sh` -> 10/21
  - `bash scripts/run-upstream-tests.sh t4208-log-magic-pathspec` -> 10/21

### Root causes identified

- `log` revision parsing fallback treated unresolved revision-like magic tokens as pathspecs whenever `--` was not present.
  - This caused commands like `git log :/detached --` and `git log :^does-not-exist` to succeed incorrectly (empty output instead of failure).
  - It also prevented ambiguity detection for `git log :/a` when `a` existed in the worktree.
- `rev-parse` accepted bare `:/` as commit-message search with empty pattern, which stole `:/` from pathspec use in `git log HEAD -- :/`.
- Shared pathspec matching treated `:/` as a literal `:` prefix rather than top-of-repo magic pathspec.

### Implemented fixes

#### 1) `grit/src/commands/log.rs`

- Added `--merge` option parsing field for compatibility (`Args::merge`) so `git log --merge -- a` parsing succeeds in this test file.
- Added log-invocation separator detection:
  - `invocation_has_double_dash_for_log()` to distinguish behavior with and without explicit `--`.
- Added Git-compatible ambiguity/error handling helpers:
  - `ambiguous_revision_or_path_error()`
  - `extract_exclude_pathspec()`
  - `is_unknown_pathspec_magic()`
  - `pathspec_candidate_exists()`
- Updated revision loop in `run()`:
  - `:` without `--` now fails as ambiguous (expected by test 11).
  - `:/<msg>` without `--` now checks for ambiguous file/revision usage and errors when both interpretations are valid (test 3).
  - `:!`, `:^`, and `:(exclude)` now fail if pathspec target does not exist instead of silently succeeding.
  - Unknown `:(magic)` now errors with `pathspec.magic` message.
  - `rev`-resolution errors are no longer silently downgraded to pathspecs for colon-prefixed magic tokens.

#### 2) `grit-lib/src/rev_parse.rs`

- Tightened `resolve_revision()` handling for `:/`:
  - now only treats `:/<non-empty>` as commit-message search.
  - bare `:/` is not resolved as a revision and can be interpreted as a pathspec when used after `--`.
- `resolve_base()` now rejects empty `:/` commit-message search pattern.
- `resolve_commit_message_search()` now rejects empty search patterns explicitly.

#### 3) `grit/src/pathspec.rs`

- Added top-level pathspec handling in `pathspec_matches()`:
  - `:/` matches from repo root (effectively matches all paths).
  - `:/<pattern>` strips magic prefix and then matches pattern content.

### Validation

- `cargo build --release` -> pass.
- `bash scripts/run-upstream-tests.sh t4208-log-magic-pathspec` -> **21/21 pass**.
- `./scripts/run-tests.sh t4208-log-magic-pathspec.sh` -> **21/21 pass**.
- Quality gates:
  - `cargo fmt` -> pass
  - `cargo clippy --fix --allow-dirty` -> pass (reverted unrelated clippy edits in `grit-lib/src/state.rs`, `grit/src/commands/blame.rs`, `grit/src/commands/config.rs`, and `grit/src/commands/update_index.rs`)
  - `cargo test -p grit-lib --lib` -> pass (96/96)

### Outcome

- `t4208-log-magic-pathspec` is now fully passing and ready to be marked complete.
