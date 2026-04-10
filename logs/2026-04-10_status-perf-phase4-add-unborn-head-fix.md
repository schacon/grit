## 2026-04-10 — phase4: remove unintended `git add` root-commit side effect

### Context

While validating status/fsmonitor and untracked-cache suites, many failures were cascading from
`git commit -m initial` unexpectedly reporting:

`error: nothing to commit, working tree clean`

Investigation showed this was not a status/fsmonitor root cause. The issue came from `git add`
creating a root commit implicitly when `HEAD` was unborn.

### Root cause

`grit/src/commands/add.rs` contained logic to auto-create an initial commit after writing the
index (`maybe_create_initial_commit_after_add`). This mutates repository history during add,
which is not Git behavior and caused tests that expect explicit first commits to fail.

### Change

- Removed the implicit post-add commit creation call from `git add` flow.
- Removed now-unused imports and helper references tied to that behavior.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo build --release -p grit-rs`
- `bash tests/t7519-status-fsmonitor.sh --run=4 -v` with release binary
  - before: setup test failed (`nothing to commit, working tree clean`)
  - after: setup test passed
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`
  - before (earlier phase baseline): 12/33
  - after: 18/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`
  - before (earlier phase baseline): 12/58
  - after: 14/58

This fix unblocked status-related suites by restoring expected add/commit lifecycle semantics.
