## t1015-read-index-unmerged

- Read `/Users/schacon/projects/grit/AGENTS.md`, the `t1015-read-index-unmerged` entry in `PLAN.md`, and upstream `git/t/t1015-read-index-unmerged.sh`.
- Ran the requested command:
  `CARGO_TARGET_DIR=/tmp/grit-build-t1015 bash scripts/run-upstream-tests.sh t1015 2>&1 | tail -40`
  which initially reported `4/6` passing against `/Users/schacon/projects/grit/target/release/grit` once the release binary was rebuilt.
- Unblocked the build first by resolving stray merge-conflict markers in `/Users/schacon/projects/grit/grit/src/main.rs`, which were preventing `cargo build --release`.
- Reproduced the two failing cases directly and confirmed:
  `git merge d-edit` aborted before writing `MERGE_HEAD` on a D/F conflict, and `git format-patch -1 d-edit` selected the wrong commit, causing `git am -3` to apply cleanly instead of entering a skip-able session.
- Updated `/Users/schacon/projects/grit/grit/src/commands/merge.rs` to prepare worktree paths safely during conflict materialization and abort cleanup, removing blocking file/directory ancestors so D/F conflicts can be recorded and later aborted.
- Updated `/Users/schacon/projects/grit/grit/src/commands/am.rs` to detect D/F conflicts during patch application and 3-way fallback, preserve the `am` session on conflict, and cleanly restore worktree/index paths during `am --skip` and abort-like resets.
- Updated `/Users/schacon/projects/grit/grit/src/commands/format_patch.rs` so `format-patch -1 <rev>` emits the named commit itself instead of treating `<rev>` as a lower bound.
- Rebuilt with `cargo build --release` and confirmed the requested upstream harness command now reports `6/6` passing for `t1015-read-index-unmerged`.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t1015 cargo fmt` successfully.
- Attempted `CARGO_TARGET_DIR=/tmp/grit-build-t1015 cargo clippy --fix --allow-dirty`, but the sandbox blocked Cargo's TCP-based lock manager setup with `Operation not permitted (os error 1)`.
- Updated `PLAN.md` and `progress.md` to mark `t1015-read-index-unmerged` complete.
