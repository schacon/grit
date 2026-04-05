# t2105-update-index-gitfile

- Time: 2026-04-05 20:29 Europe/Berlin
- Scope: re-verify `git/t/t2105-update-index-gitfile.sh`, update plan/progress bookkeeping, and ship the validated result.
- Observed behavior: the isolated upstream test and `scripts/run-upstream-tests.sh t2105` both pass 4/4 against the current `target/release/grit`.
- Code changes: none required for this task; `grit/src/commands/update_index.rs` already resolves `.git` directories and gitfiles and stages submodules as `160000` gitlinks.
- Validation:
  - `cargo build --release -p grit-rs`
  - `CARGO_TARGET_DIR=/tmp/grit-build-t2105 bash scripts/run-upstream-tests.sh t2105`
  - `CARGO_TARGET_DIR=/tmp/grit-build-t2105 cargo fmt --all`
- Notes: `cargo test --workspace` and `./tests/harness/run.sh` were not run for this task.
