## t1514-rev-parse-push

### Goal
Make `tests/t1514-rev-parse-push.sh` fully pass.

### Initial state
- `./scripts/run-tests.sh t1514-rev-parse-push.sh` reported **3/9**.
- Failing cases:
  - setup (`origin/main` not resolvable after `git push origin HEAD`)
  - `@{push}` with `push.default=simple`
  - `@{push}` with `push.default=current/matching/pushremote/push refspecs`

### Root causes
1. `push` did not update local tracking refs (`refs/remotes/<remote>/<branch>`) after successful pushes.
2. `@{push}` resolution in `rev-parse` lacked push-mode config semantics:
   - `push.default`
   - `branch.<name>.pushRemote`
   - `remote.pushDefault`
   - `remote.<name>.push` refspec mapping.
3. DWIM ref resolution in `rev-parse` did not include `refs/<name>` fallback required by worktree-related tests.

### Code changes
- `grit/src/commands/push.rs`
  - Added `update_remote_tracking_ref()` and invoked it after successful updates/deletes.
  - Local remote-tracking refs now mirror push results, enabling immediate `origin/main` and related resolution in tests.
- `grit-lib/src/rev_parse.rs`
  - Implemented richer `resolve_push_ref()` logic:
    - honors `push.default` (`nothing`, `simple`, `upstream/tracking`, `current`, `matching`)
    - honors `branch.<name>.pushRemote` and `remote.pushDefault`
    - maps through `remote.<name>.push` refspecs (including wildcard patterns).
  - Added `refs/<name>` candidate in DWIM fallback list.
  - Added per-worktree ref namespace resolution (`main-worktree/*`, `worktrees/<id>/*`, `worktree/*`) with ambiguity warning support.
- `grit/src/commands/reflog.rs`
  - Added cross-worktree reflog location mapping for `main-worktree/*` and `worktrees/<id>/*`.
- `grit/src/commands/for_each_ref.rs`
  - Added common-dir aware ref collection for linked worktrees while preserving per-worktree namespace isolation.

### Validation
- `cargo fmt`
- `cargo build --release -p grit-rs`
- `rm -rf /workspace/tests/trash.t1514-rev-parse-push && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1514-rev-parse-push.sh` → **9/9 pass**
- `./scripts/run-tests.sh t1514-rev-parse-push.sh` → **9/9 pass**
- `cargo clippy --fix --allow-dirty` (reverted unrelated edits)
- `cargo test -p grit-lib --lib` → **96 passed**
