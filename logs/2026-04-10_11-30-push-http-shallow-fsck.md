# 2026-04-10 — phase 5 HTTP shallow push baseline/fix (`t5542`)

## Scope

- Continue push implementation plan Phase 5 HTTP path validation.
- Re-run HTTP-targeted suites to capture current failures.
- Fix the failing `t5542-push-http-shallow` scenario.

## Baseline

- `./scripts/run-tests.sh t5549-fetch-push-http.sh` → `3/3` (already green)
- `./scripts/run-tests.sh t5542-push-http-shallow.sh` → `2/3` (failing test 2)

Failing test (`t5542.2`) symptom:

- Push to a shallow bare repo over HTTP succeeded, but follow-up `git fsck` in the remote repo failed with:
  - `missing tree <oid> (referenced by <commit>)`

## Root cause

- The failing repo is intentionally shallow (`.git/shallow` exists and boundary commits have missing parents).
- Grit `fsck` reachability traversal in `grit/src/commands/fsck.rs` always walked commit parents.
- In shallow repos, parent links past shallow boundaries must not be traversed.
- Traversing beyond shallow boundaries caused false-positive missing-object diagnostics during `fsck` after HTTP push.

## Code changes

### `grit/src/commands/fsck.rs`

- Added `load_shallow_boundaries(git_dir: &Path) -> HashSet<ObjectId>`.
- In `walk_reachable`:
  - load shallow boundary OIDs from `$GIT_DIR/shallow`,
  - when visiting a commit, still enqueue its tree,
  - but **skip enqueueing commit parents** if the commit is a shallow boundary.

This matches Git shallow semantics for connectivity checks.

## Validation

- `cargo build --release -p grit-rs` ✅
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs` ✅ (then reverted unrelated formatting-only files)
- `cargo test -p grit-lib --lib` ✅ (`166 passed`)
- `./scripts/run-tests.sh t5549-fetch-push-http.sh` ✅ `3/3`
- `./scripts/run-tests.sh t5542-push-http-shallow.sh` ✅ `3/3`
- `./scripts/run-tests.sh t5545-push-options.sh` ✅ `13/13` (regression guard)
- `./scripts/run-tests.sh t5516-fetch-push.sh` ✅/❌ `52/124` (broad baseline only, not target-complete for this slice)

