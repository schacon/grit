## t1415-worktree-refs

### Goal
Make `tests/t1415-worktree-refs.sh` fully pass.

### Initial state
- `./scripts/run-tests.sh t1415-worktree-refs.sh` reported **4/10**.
- Failing cases:
  - `FAIL 2: refs/worktree are per-worktree`
  - `FAIL 3: resolve main-worktree/HEAD`
  - `FAIL 4: ambiguous main-worktree/HEAD`
  - `FAIL 6: ambiguous worktrees/xx/HEAD`
  - `FAIL 7: reflog of main-worktree/HEAD`
  - `FAIL 10: for-each-ref from linked worktree`

### Root causes
1. `rev-parse` did not resolve worktree-specific ref namespaces:
   - `worktree/*`
   - `main-worktree/*`
   - `worktrees/<id>/*`
2. `rev-parse` had no ambiguity warning when those names collided with
   `refs/heads/<name>`.
3. `reflog` only read logs from the current worktree gitdir, so cross-worktree
   reflog paths (`main-worktree/*`, `worktrees/<id>/*`) returned empty output.
4. `for-each-ref` in a linked worktree enumerated only local worktree refs and
   missed shared branch refs from the common gitdir.

### Code changes
- `grit-lib/src/rev_parse.rs`
  - Added worktree-aware ref resolution for:
    - current-worktree aliases (`worktree/<name>` → `refs/worktree/<name>`)
    - `main-worktree/<ref>`
    - `worktrees/<id>/<ref>`
  - Added `refs/<spec>` DWIM fallback for names like `worktree/foo`.
  - Added Git-style ambiguity warning when a worktree ref path collides with
    `refs/heads/<spec>`.
- `grit/src/commands/reflog.rs`
  - Added cross-worktree reflog location mapping so `reflog show/exists` read
    from:
    - common dir for `main-worktree/<ref>`
    - `common/worktrees/<id>` for `worktrees/<id>/<ref>`
- `grit/src/commands/for_each_ref.rs`
  - Added common-dir awareness for linked worktrees:
    - include shared refs from common dir
    - retain current-worktree refs from linked worktree gitdir
    - exclude per-worktree namespaces from common-dir import

### Validation
- `cargo fmt`
- `cargo build --release -p grit-rs`
- `rm -rf /workspace/tests/trash.t1415-worktree-refs && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1415-worktree-refs.sh` → **10/10 pass**
- `./scripts/run-tests.sh t1415-worktree-refs.sh` → **10/10 pass**
- `cargo clippy --fix --allow-dirty` (reverted unrelated edits)
- `cargo test -p grit-lib --lib` → **96 passed**
