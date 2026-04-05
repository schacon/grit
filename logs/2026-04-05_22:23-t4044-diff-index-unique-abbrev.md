# t4044-diff-index-unique-abbrev

- Claimed the `t4044-diff-index-unique-abbrev` task and read `AGENTS.md`, `PLAN.md`, and `git/t/t4044-diff-index-unique-abbrev.sh`.
- Reproduced the failure in an isolated upstream workdir. The failing output was `index 51d2738..51d2738 100644`, which is ambiguous.
- Updated `grit/src/commands/diff.rs` so patch headers use repository-aware unique object abbreviation instead of fixed 7-character slices.
- Updated `grit/src/commands/diff_index.rs` the same way so `diff-index -p` stays aligned with the shared patch behavior.
- Rebuilt `grit` with `CARGO_TARGET_DIR=/tmp/grit-build-t4044 cargo build --release -p grit-rs` and refreshed `target/release/grit`.
- Verified `t4044` directly in `/tmp/grit-upstream-workdir/t` and then reran `CARGO_TARGET_DIR=/tmp/grit-build-t4044 bash scripts/run-upstream-tests.sh t4044-diff-index-unique-abbrev 2>&1 | tail -40`, which reported 2/2 passing.
- Updated `PLAN.md`, `progress.md`, and `test-results.md` for the completed task.
