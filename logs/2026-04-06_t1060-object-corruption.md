# 2026-04-06 — t1060-object-corruption

## Scope
- Target file: `tests/t1060-object-corruption.sh`
- Starting status in plan: `11/17` passing (6 failing)
- Goal: make `t1060-object-corruption` fully pass without modifying tests.

## Failures addressed
1. `clone --no-local --bare` accepted corrupted/missing/misnamed objects (transport path did not validate object graph integrity for bare clones).
2. `rev-list --objects` treated the canonical empty tree object as missing in repos where it is intentionally absent from loose storage.

## Implemented fixes

### `grit/src/commands/clone.rs`
- Added explicit repository object validation before ref copy in `--bare` clone mode:
  - walks all copied refs (heads + tags),
  - recursively validates commit/tree/blob objects,
  - detects both missing and corrupt/misnamed objects.
- Introduced helper functions:
  - `validate_repo_objects_for_refs`
  - `validate_object_graph`
- This enforces failure for:
  - corrupted loose object content (zlib/decode failures),
  - missing referenced objects,
  - misnamed object files where OID-to-content mismatch appears as graph corruption.

### `grit-lib/src/rev_list.rs`
- Added canonical empty tree fallback handling in object traversal:
  - when tree OID is `4b825dc642cb6eb9a060e54bf8d69288fbee4904` and absent from ODB,
    treat it as an empty tree instead of missing.
- Applied fallback in both:
  - `collect_tree_objects_filtered`
  - `flatten_tree`
- This aligns `rev-list --objects` behavior with Git for empty-root commits.

## Validation
- `cargo fmt` ✅
- `cargo build --release -p grit-rs` ✅
- `rm -rf tests/trash.t1060-object-corruption && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1060-object-corruption.sh` ✅ `17/17` pass (1 expected-failure stub skipped by harness accounting)
- `./scripts/run-tests.sh t1060-object-corruption.sh` ✅ `17/17`
- Regression checks:
  - `./scripts/run-tests.sh t1302-repo-version.sh` ✅ `18/18`
  - `./scripts/run-tests.sh t1090-sparse-checkout-scope.sh` ✅ `7/7`
- `cargo clippy --fix --allow-dirty && cargo test -p grit-lib --lib` ✅ (unrelated clippy edits reverted)

## Tracking updates
- `PLAN.md`: marked `t1060-object-corruption` complete (`17/17`).
- `progress.md`: updated counts to Completed `77`, Remaining `690`, Total `767`; added `t1060` to recent completions.
- `test-results.md`: appended build/test evidence for `t1060` completion and regressions.
