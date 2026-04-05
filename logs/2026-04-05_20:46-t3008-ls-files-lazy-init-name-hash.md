# t3008-ls-files-lazy-init-name-hash

- Task: verify and fix the single remaining failure for `t3008-ls-files-lazy-init-name-hash`.
- Result: `grit/src/main.rs` already contained the needed `test-tool online-cpus` and `test-tool lazy-init-name-hash` support; rebuilding the stale `target/release/grit` binary made the upstream file pass.
- Actions:
  - Read `AGENTS.md`, `plan.md`, `git/t/t3008-ls-files-lazy-init-name-hash.sh`, and `git/Documentation/git-ls-files.adoc`.
  - Reproduced the failing file-level upstream result with `CARGO_TARGET_DIR=/tmp/grit-build-t3008 bash scripts/run-upstream-tests.sh t3008`.
  - Confirmed the source already contained both helper implementations, but the built `target/release/grit` binary was stale.
  - Rebuilt the actual harness binary with `cargo build --release -p grit-rs`.
  - Re-ran `CARGO_TARGET_DIR=/tmp/grit-build-t3008 bash scripts/run-upstream-tests.sh t3008 2>&1 | tail -40`.
- Verification:
  - Upstream result after rebuild: `Files where ALL tests pass: 1`
  - Test summary after rebuild: `Tests: 1 (pass: 1, fail: 0)`
- Tracking updates:
  - Marked `t3008-ls-files-lazy-init-name-hash` as complete in `PLAN.md`.
  - Added this task log and updated `progress.md` recent completions.
- Stop reason: `complete`
