# t4036-format-patch-signer-mime

- Time: 2026-04-05 21:25 Europe/Berlin
- Task: verify and finish `t4036-format-patch-signer-mime`
- Result: upstream rerun already passes 5/5 against `target/release/grit`

## Actions

- Read `AGENTS.md`, `plan.md`, `git/t/t4036-format-patch-signer-mime.sh`, and the related `git-format-patch` documentation.
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t4036 bash scripts/run-upstream-tests.sh t4036-format-patch-signer-mime 2>&1 | tail -40`.
- Inspected `grit/src/commands/format_patch.rs` to confirm the existing behavior:
  - non-ASCII signoff names force MIME headers for non-attachment output
  - `--attach` output emits a single `MIME-Version` header
- Ran `CARGO_TARGET_DIR=/tmp/grit-build-t4036 cargo fmt --all 2>/dev/null; true`.
- Updated `plan.md`, `progress.md`, and `test-results.md` to reflect the verified green state.

## Notes

- No Rust source changes were required for this task.
- `git add` / `git commit` / `git push` are blocked in this sandbox because writes inside `/Users/schacon/projects/grit/.git` fail with `Operation not permitted` when creating `index.lock`.
