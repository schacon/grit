## t4067-diff-partial-clone (claim/baseline)

### Claim

- Claimed next Diff target from plan: `t4067-diff-partial-clone`.
- Updated plan status from `[ ]` to `[~]`.

### Baseline

- `./scripts/run-tests.sh t4067-diff-partial-clone.sh` -> 0/9 passing.
- `bash scripts/run-upstream-tests.sh t4067-diff-partial-clone` -> 2/9 passing.

### Initial failing assertions snapshot (upstream)

- 1: `git show` on partial-clone bare client should batch missing blob fetches into one negotiation.
- 2: `git diff HEAD^ HEAD` should batch missing blob fetches into one negotiation.
- 3: `git diff` should avoid fetching unchanged same-OID blobs.
- 4: `git diff` should skip fetching gitlink/submodule pseudo-blobs.
- 5: `git diff --raw -M` should batch rename-detection blob prefetch.
- 8: exact-rename case should avoid any fetch when inexact detection is unnecessary.
- 9: `--break-rewrites -M` should fetch only when required and batch when it does.

### Implementation details

- `grit/src/commands/diff.rs`
  - Added bare-repo argv recovery:
    - `reclassify_bare_revision_paths(...)` now reclassifies tokens initially parsed as paths back into revisions when running in bare repositories without `--`.
    - This fixes `git -C <bare> diff HEAD^ HEAD` incorrectly producing empty output / missing trace files.
  - Added lazy prefetch integration:
    - `maybe_prefetch_for_tree_entries(...)` gathers needed blob OIDs for patch/stat/numstat/check/word-diff output, rename/copy similarity checks, and break-rewrite detection.
    - Skips gitlinks (`160000`) and exact-rename fast-paths where fetch is unnecessary.
  - Wired prefetch before tree-vs-tree output for 2-rev diffs.

- `grit/src/commands/checkout.rs`
  - Added partial-clone lazy hydration in `write_blob_to_worktree(...)`:
    - When a blob OID is missing locally, call `partial_clone::maybe_fetch_missing_objects(...)` before reading blob data for checkout/reset flows.
    - Fixes checkout/reset paths used by test 6 (`checkout HEAD~1 bar` then reset/break-rewrite flow).

- `grit/src/commands/add.rs`
  - Enhanced embedded repo (gitlink) HEAD resolution in `stage_gitlink(...)`:
    - Supports both `.git/` directory and `.git` gitfile indirection.
    - Resolves referenced gitdir for submodule-style repositories before reading HEAD/ref.
    - Fixes submodule/gitlink staging reliability in partial-clone submodule scenario.

### Validation

- `cargo build --release` ✅
- `bash scripts/run-upstream-tests.sh t4067-diff-partial-clone` ✅ 9/9
- Direct verbose upstream run:
  - `cd /tmp/grit-upstream-workdir/t && GIT_BUILD_DIR=/tmp/grit-upstream-workdir TEST_NO_MALLOC_CHECK=1 TAR=tar bash ./t4067-diff-partial-clone.sh -v -x -i`
  - ✅ 9/9 with expected `fetch> done` and `want <oid>` packet-trace assertions
- `./scripts/run-tests.sh t4067-diff-partial-clone.sh` reports 0/9 in this local mirror due known `tests/test-lib.sh` incompatibility (`git config -C` argument order); upstream harness is authoritative for this target.
- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated autofixes)
- `cargo test -p grit-lib --lib` ✅ 96/96
