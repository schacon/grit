## Task: t4206-log-follow-harder-copies

### Claim
- Claimed from `PLAN.md` after completing `t4107-apply-ignore-whitespace`.
- Initial status: `3/7` passing in local harness.

### Baseline
- `./scripts/run-tests.sh t4206-log-follow-harder-copies.sh` → `3/7` then `4/7` after first parsing fixes.
- Direct local repro:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4206-log-follow-harder-copies.sh`
  - failures included:
    - `find the copy path0 -> path1 harder`
    - output shape mismatch for `--follow --name-status --pretty=format:%s`
    - `log --follow -B` rejected (`-B` unsupported)
    - occasional `corrupt object: commit missing tree header` due revision/pathspec misclassification in `log` argument handling.

### Root causes
1. `log` parsed all positional arguments as revisions; path arguments like `path1` were incorrectly sent to revision resolution in follow mode.
2. `--follow` path tracking used rename-only logic and did not emit copy (`C100`) status records for copy history.
3. `-B` (`--break-rewrites`) was not accepted by the command parser.
4. In `--follow --name-status --pretty=format:%s`, output separation/newline behavior did not match expected git formatting.
5. `run_no_walk` did not peel tag inputs to commit objects before parsing commit data.

### Implemented fixes
- `grit/src/commands/log.rs`:
  - Added `-B` / `--break-rewrites[=<n>]` option parsing for compatibility.
  - Added argument classifier that separates revision-like tokens from pathspec-like tokens in `--follow` mode when `--` is not present.
  - Updated commit start/exclude resolution to use classified revision tokens.
  - Updated follow filtering to use copy-aware detection (`detect_copies(..., find_copies_harder=true, source_tree_entries)`), and to track path transitions across `Renamed` and `Copied` entries.
  - Added recursive source tree flattener for copy detection inputs.
  - Added follow-specific name-status emission helper producing:
    - `C<score>\t<old>\t<new>` for copies/renames
    - `<status>\t<path>` for other statuses.
  - Added follow-specific entry selector per commit so `--follow --name-status` reports the tracked path’s effective change (matching expected `C100 path0 path1`, then `M path0`, then `A path0`).
  - Adjusted pretty-format separator behavior so `--pretty=format:%s` + name-status outputs one blank line between commits and no extra trailing blank commit separator.
  - Updated `run_no_walk` to peel tag objects to commits before `parse_commit`.

### Validation
- `cargo build --release` ✅
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4206-log-follow-harder-copies.sh` ✅ `7/7`
- `./scripts/run-tests.sh t4206-log-follow-harder-copies.sh` ✅ `7/7`
- `bash scripts/run-upstream-tests.sh t4206-log-follow-harder-copies` ✅ `7/7`
- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅ (unrelated autofixes reverted)
- `cargo test -p grit-lib --lib` ✅

### Outcome
- `t4206-log-follow-harder-copies` is now complete and marked done in `PLAN.md` (`7/7`).
