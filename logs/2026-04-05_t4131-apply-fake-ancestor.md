# t4131-apply-fake-ancestor

- Claimed and worked the `t4131-apply-fake-ancestor` plan item.
- Read `AGENTS.md`, `plan.md`, `git/t/t4131-apply-fake-ancestor.sh`, and `git/Documentation/git-apply.adoc`.
- Reproduced the failure with `CARGO_TARGET_DIR=/tmp/grit-build-t4131 bash scripts/run-upstream-tests.sh t4131-apply-fake-ancestor 2>&1 | tail -40` and then directly under `/tmp/grit-upstream-workdir/t` to inspect the failing subdirectory case.
- Implemented `git apply --build-fake-ancestor=<file>` in [`grit/src/commands/apply.rs`](/Users/schacon/projects/grit/grit/src/commands/apply.rs):
  wrote a real temporary index from patch pre-image blobs,
  resolved abbreviated object IDs from the local object store,
  preserved repository-root relative paths from subdirectories,
  and returned immediately after writing the fake ancestor so the worktree stays unchanged.
- Updated the existing `add -e` call site for the new apply argument and fixed small compile blockers already present in this worktree so `target/release/grit` could be rebuilt.
- Cleared a stale `/tmp/grit-upstream-workdir` with `find ... -delete` because the harness cleanup left old state behind in this sandbox.
- Verified the final result with `CARGO_TARGET_DIR=/tmp/grit-build-t4131 bash scripts/run-upstream-tests.sh t4131-apply-fake-ancestor 2>&1 | tail -40`, which reported 3/3 passing.
