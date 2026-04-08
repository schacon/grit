# t3102-ls-tree-wildcards — 2026-04-05

## Task

Fix the remaining failing test in `t3102-ls-tree-wildcards.sh`.

## Investigation

- Read `AGENTS.md`, the `t3102-ls-tree-wildcards` entry in `plan.md`, and upstream `git/t/t3102-ls-tree-wildcards.sh`.
- Ran the requested upstream harness command:
  `CARGO_TARGET_DIR=/tmp/grit-build-t3102 bash scripts/run-upstream-tests.sh t3102 2>&1 | tail -40`
- Verified the failing case was the negated pathspec test and traced `ls-tree` path filtering in `grit/src/commands/ls_tree.rs`.

## Findings

- `ls-tree` still used ad hoc literal-prefix filtering, so `:(exclude)` pathspec magic was ignored.
- Recursive wildcard matching also needed to preserve the existing literal-directory behavior for names containing glob metacharacters such as `a[a]`.
- The repository already had concurrent pathspec-related edits in `grit/src/pathspec.rs`; one used Rust-2024 `let` chains and had to be rewritten to Rust-2021 syntax so the requested rebuild could succeed.
- The upstream harness executes `target/release/grit`, so validation required pointing that path at the fresh `/tmp/grit-build-t3102/release/grit` binary.

## Changes

- Reworked `ls-tree` pathspec filtering in `grit/src/commands/ls_tree.rs` to:
  - parse supported pathspec magic via the shared parser,
  - resolve pathspecs relative to the worktree/cwd,
  - apply include/exclude filtering during traversal,
  - keep recursive literal matching working for directory names like `a[a]`.
- Rewrote the new `parse_magic_pathspec()` branch in `grit/src/pathspec.rs` to avoid Rust-2024-only `let` chains.
- Flipped the now-fixed negated-pathspec case from `test_expect_failure` to `test_expect_success` in:
  - `git/t/t3102-ls-tree-wildcards.sh`
  - `tests/t3102-ls-tree-wildcards.sh`

## Validation

- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 cargo build --release -p grit-rs`
- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 bash scripts/run-upstream-tests.sh t3102`
  - Result: 4/4 passing.
- `./scripts/run-tests.sh t3102-ls-tree-wildcards.sh` — **4/4 passing**
- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 cargo fmt`
- `CARGO_TARGET_DIR=/tmp/grit-build-t3102 cargo clippy --fix --allow-dirty`
  - Blocked by sandbox: `failed to bind TCP listener to manage locking (Operation not permitted)`.

## Follow-up

- Updated `PLAN.md` entry to `[x]` at 4/4; `progress.md` and `test-results.md` refreshed.
