## t4059-diff-submodule-not-initialized (claim/baseline)

### Claim
- Claimed next Diff target from plan: `t4059-diff-submodule-not-initialized`.
- Updated plan status from `[ ]` to `[~]`.

### Baseline
- `./scripts/run-tests.sh t4059-diff-submodule-not-initialized.sh` -> 1/8 passing.
- `bash scripts/run-upstream-tests.sh t4059-diff-submodule-not-initialized` -> 1/8 passing.

### Notes for next implementation step
- Focus area is diff behavior for uninitialized submodules (patch/raw/stat variants).
- Next actions:
  1. run direct upstream test script with `-v` to capture first concrete failing assertions.
  2. inspect `grit/src/commands/diff.rs`, `grit/src/commands/diff_index.rs`, and `grit/src/commands/diff_tree.rs` submodule rendering paths.
  3. implement missing uninitialized-submodule diff semantics and re-test upstream/local.

## Implementation and validation (completed)

### Code changes
- `grit submodule add`:
  - accepts a pre-existing empty destination directory and reuses it for clone destination.
  - writes canonical `.git` gitfile content pointing at the separate module gitdir (`.git/modules/<name>`).
- `grit submodule update`:
  - now accepts `--checkout` for compatibility.
  - re-attaches missing submodule working trees when the module gitdir exists by recreating the gitfile and setting `core.worktree`.
- `grit commit -a`:
  - preserves existing gitlink index entries when the submodule worktree is missing, instead of deleting them from the index.
- `grit mv`:
  - allows renaming a tracked path that is an empty directory on disk when tracked children exist in the index (required for submodule directory moves).
- `grit diff-tree -p --submodule=log`:
  - adds parser support for `--submodule=<format>`.
  - renders submodule summary lines for gitlink entries in patch mode (`new submodule`, range summaries, `commits not present`).
  - decodes submodule commit subjects with encoding-aware handling (`encoding` header / legacy commit encodings) via `encoding_rs`.
  - suppresses `.gitmodules` patch hunks in `--submodule=log` mode.
  - coalesces pure gitlink rename/delete+add pairs to avoid duplicate delete/add summary lines for moved submodules.

### Test evidence
- Baseline:
  - `./scripts/run-tests.sh t4059-diff-submodule-not-initialized.sh` → 1/8.
  - `bash scripts/run-upstream-tests.sh t4059-diff-submodule-not-initialized` → 1/8.
- Final validation:
  - `cargo build --release` → pass.
  - `bash scripts/run-upstream-tests.sh t4059-diff-submodule-not-initialized` → **8/8 pass**.
  - Direct upstream verbose run:
    - `cd /tmp/grit-upstream-workdir/t && GIT_BUILD_DIR=/tmp/grit-upstream-workdir TEST_NO_MALLOC_CHECK=1 TAR=tar bash ./t4059-diff-submodule-not-initialized.sh -v`
    - result: **8/8 pass** with expected submodule log output for removed worktrees, uninitialized clones, and moved submodule paths.
  - Quality gates:
    - `cargo fmt` → pass.
    - `cargo clippy --fix --allow-dirty` → pass (unrelated autofixes reverted).
    - `cargo test -p grit-lib --lib` → pass (96/96).
  - Local targeted mirror:
    - `./scripts/run-tests.sh t4059-diff-submodule-not-initialized.sh` → **8/8 pass**.
