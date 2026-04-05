## Task: t4131-apply-fake-ancestor

### Claim
- Claimed after completing `t4125-apply-ws-fuzz`.
- Marked as `[~]` in `PLAN.md`.

### Baseline
- `./scripts/run-tests.sh t4131-apply-fake-ancestor.sh` reports `1/3` passing (`2` left).

### Next
- Reproduced failures:
  - `grit apply` rejected `--build-fake-ancestor` as unknown option.
  - After adding option parsing, subdirectory invocation still failed while reading non-local paths.

### Root cause
- `grit apply` had no implementation of `--build-fake-ancestor`.
- Fake ancestor building must use patch index metadata (`index <old>..<new>`) to resolve old blobs and write an index file, independent of current working directory path layout.

### Changes implemented
- `grit/src/commands/apply.rs`:
  - Added CLI argument parsing for `--build-fake-ancestor=<file>`.
  - Implemented `build_fake_ancestor_index(...)`:
    - Parses old blob OIDs from patch `index` headers.
    - Resolves abbreviated OIDs against repository object database.
    - Builds synthetic `IndexEntry` rows with path/mode/OID from patch metadata.
    - Writes an index file to the requested path.
  - Invoked fake-ancestor construction before apply/check execution paths.
  - Kept behavior non-destructive: writing fake ancestor file does not alter worktree/index apply semantics.

### Validation
- `cargo fmt` ✅
- `cargo build --release` ✅
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4131-apply-fake-ancestor.sh` ✅ `3/3`
- `./scripts/run-tests.sh t4131-apply-fake-ancestor.sh` ✅ `3/3`
- `cargo test -p grit-lib --lib` ✅
- `cargo clippy --fix --allow-dirty` ✅ (unrelated edits reverted)

### Result
- `t4131-apply-fake-ancestor` now fully passes (`3/3`).
