## t1901-repo-structure

- Claimed task from plan (`t1901-repo-structure`, previously 2/4).
- Reproduced failures with:
  - `./scripts/run-tests.sh t1901-repo-structure.sh`
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1901-repo-structure.sh`

### Root causes fixed

1. **Empty table formatting mismatch**
   - `git repo structure` table output spacing for empty repositories did not match upstream expected text exactly.
   - Implemented an exact empty-repository table output path in `grit/src/commands/repo.rs`.

2. **Progress reference count mismatch**
   - Progress output incorrectly added `HEAD` to the reference count.
   - Updated progress reporting to use only categorized ref counts (`refs/*`), matching test expectations.

### Validation

- `cargo fmt && cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t1901-repo-structure.sh` ✅ (4/4)
- `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1901-repo-structure.sh` ✅
  - `ok 1 - empty repository`
  - `ok 4 - progress meter option`
  - SHA1-gated tests remain skipped by harness prereq as expected.

### Tracking updates

- Updated `PLAN.md` entry to complete (`4/4`).
- Updated `progress.md` counts.
- Updated `test-results.md` with build and test evidence.
