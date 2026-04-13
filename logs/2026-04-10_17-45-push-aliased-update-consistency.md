## 2026-04-10 17:45 — t5516 aliased-ref push consistency

### Goal
Address `t5516-fetch-push.sh` failure:
- `push into aliased refs (inconsistent)` (test 84).

### Root cause
- Push prepared updates for both destination refs (`foo` and `bar`) even when
  one is a symbolic ref to the other in the remote repository.
- For inconsistent updates, Git rejects the push before applying updates with:
  `refusing inconsistent update ...`.
- For consistent updates, writing both refs directly would destroy the symbolic
  ref in file-based storage (`refs/heads/bar` would become a direct ref file).

### Change
- Added `reject_or_drop_aliased_remote_updates(remote_git_dir, &mut updates)` in
  `grit/src/commands/push.rs`:
  - Detect remote symbolic refs among planned destination refs.
  - If symbolic ref update disagrees with target update old/new OIDs, `bail!`
    with:
    `refusing inconsistent update between symref '<sym>' and its target '<dst>'`.
  - If consistent, drop the symbolic-ref update and keep only the target update
    to preserve the remote symref.
- Invoked this check just before recurse-submodule-only early return and before
  applying any updates.

### Validation
- `cargo fmt` — pass
- `cargo check` — pass
- `cargo clippy --fix --allow-dirty` — pass (reverted unrelated edits)
- `cargo test -p grit-lib --lib` — pass
- `cargo build --release -p grit-rs` — pass
- `./scripts/run-tests.sh t5516-fetch-push.sh` — **60/124** (up from 59/124)
  - `push into aliased refs (inconsistent)` now passes.

