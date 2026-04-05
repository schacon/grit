# t2015-checkout-unborn

- Read `/Users/schacon/projects/grit/AGENTS.md`, the `t2015-checkout-unborn` entry in `PLAN.md`, and upstream `git/t/t2015-checkout-unborn.sh`.
- Ran the requested command:
  `CARGO_TARGET_DIR=/tmp/grit-build-t2015 bash scripts/run-upstream-tests.sh t2015 2>&1 | tail -40`
  which reported `6/6` passing against `/Users/schacon/projects/grit/target/release/grit`.
- Inspected `grit/src/commands/checkout.rs` and confirmed the current source already contains the unborn-branch handling required by `t2015`:
  `create_and_switch_branch()` preserves the unborn-branch fast path and `create_orphan_branch()` updates `HEAD` without creating the branch ref.
- Built the current source with `CARGO_TARGET_DIR=/tmp/grit-build-t2015 cargo build --release`.
- Re-ran the requested upstream harness command and confirmed `t2015` still passes `6/6`.
- Ran `cargo fmt` successfully.
- Attempted `CARGO_TARGET_DIR=/tmp/grit-build-t2015 cargo clippy --fix --allow-dirty`, but the sandbox blocked Cargo before linting with:
  `error: failed to bind TCP listener to manage locking`
  `Caused by: Operation not permitted (os error 1)`
- Updated `PLAN.md` and `progress.md` to mark `t2015-checkout-unborn` complete.
