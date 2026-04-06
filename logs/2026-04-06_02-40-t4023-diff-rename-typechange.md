## Task: t4023-diff-rename-typechange

### Claim
- Claimed after completing `t4138`.
- Baseline in tracked harness: `0/4` passing.

### Baseline failures
- `setup` failed in this mirror due missing fixture file at `tests/../Makefile`.
- `cross renames to be detected for regular files` failed as a downstream consequence of setup (missing `five`/`six` tags).
- `moves and renames` failed because `diff-tree --name-status -B -M` emitted `T\tfoo` instead of expected `T100\tfoo`.

### Root causes
1. `commit -a` auto-staging used `Path::exists()`, which follows symlinks. Broken symlinks were treated as missing and dropped from the index during commit flows used by this test.
2. `diff-tree` did not parse `-B`/`--break-rewrites`, and `name-status` formatting did not emit `T100` for typechange output under break-rewrite mode.
3. Harness fixture mismatch in this environment: `../Makefile` is absent in both `tests/` and isolated upstream workdir root unless copied manually.

### Implementation
- Updated `grit/src/commands/commit.rs`:
  - `auto_stage_tracked` now uses `fs::symlink_metadata` directly instead of `exists()`, preserving symlink entries (including dangling symlinks) during `commit -a`.
- Updated `grit/src/commands/diff_tree.rs`:
  - Added parsing support for `-B`, `-B<n>`, `--break-rewrites`, and `--break-rewrites=<n>`.
  - Wired `-B` with `-M` behavior by enabling copy detection threshold when break-rewrite is requested alongside rename detection.
  - Added break-rewrite preprocessing for modified/typechanged-only diffs before rename/copy detection.
  - Updated `--name-status` output to emit `T100\t<path>` for typechange entries in break-rewrite mode (matching `t4023` expectations).

### Validation
- `cargo build --release` ✅
- Local script:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4023-diff-rename-typechange.sh` ✅ `2/4` (remaining failures are fixture/setup-related)
- Tracked harness:
  - `./scripts/run-tests.sh t4023-diff-rename-typechange.sh` ✅ `2/4`
- Upstream harness:
  - `bash scripts/run-upstream-tests.sh t4023-diff-rename-typechange` ✅ `2/4` (same missing root `Makefile` fixture in runner)
- Fixture-adjusted upstream confirmation:
  - `cp /tmp/grit-upstream-workdir/t/Makefile /tmp/grit-upstream-workdir/Makefile`
  - `cd /tmp/grit-upstream-workdir/t && ... bash ./t4023-diff-rename-typechange.sh` ✅ `4/4`

### Regressions
- `./scripts/run-tests.sh t4206-log-follow-harder-copies.sh` ✅ `7/7`
- `./scripts/run-tests.sh t4072-diff-max-depth.sh` ✅ `76/76`

### Quality gates
- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated autofixes)
- `cargo test -p grit-lib --lib` ✅

### Status
- Functionality is complete for `t4023`; tracked as partial in this mirror due missing `../Makefile` fixture semantics in test harness/workdir setup.
