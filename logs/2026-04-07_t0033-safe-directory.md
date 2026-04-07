# t0033-safe-directory — 2026-04-07

## Goal

Make `tests/t0033-safe-directory.sh` fully pass by aligning safe-directory rejection/acceptance behavior with upstream Git, including command-line/env/global/include/path-glob cases and local clone source safety checks under `GIT_TEST_ASSUME_DIFFERENT_OWNER=1`.

## Baseline

- Initial run in this session:
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t0033-safe-directory.sh`
  - Result: **5/22** then **11/22** while iterating.
- Primary observed mismatch:
  - Early errors were emitted as:
    - `error: detected dubious ownership in repository at ...`
  - Upstream tests expect rejection to present through “not a git repository … dubious ownership” shape in command contexts that go through discovery wrappers.
- Additional unstable signal:
  - A parallel accidental duplicate test invocation corrupted the working directory for one run (`getcwd() failed`); reran from clean trash state.

## Root cause

1. **Error shape mismatch**
   - Repository discovery and command-level wrappers expected “not a git repository: <reason>” style for discovery failures.
   - Returning raw `DubiousOwnership` directly produced the wrong top-level error string for several `expect_rejected_dir` assertions.

2. **Local clone source check path**
   - `clone` source opening and safety behavior needed to rely on discovery semantics for local source paths (`source`, `source/.git`, bare source), rather than partial direct-open paths.

3. **Test isolation sensitivity**
   - `t0033` is sensitive to stale `trash.t0033-safe-directory` and `bin.t0033-safe-directory`; stale state can skew observed failures.

## Code changes

### `grit-lib/src/config.rs`

- Kept improved `GIT_CONFIG_PARAMETERS` token parsing that correctly handles single-quoted parameter streams used in `safe.directory` CLI propagation.

### `grit-lib/src/error.rs`

- Kept explicit `DubiousOwnership(String)` error variant for ownership-denied cases.

### `grit-lib/src/repo.rs`

- Kept safe-directory enforcement in discovery/open paths for:
  - gitfile indirection
  - `.git` directory repositories
  - implicit bare repositories
- Kept matching helpers for safe-directory normalization and pattern matching (`*`, `~/`, `/*`, `.` semantics).
- Kept `safe.bareRepository` protected config behavior via repository-context config loading.

### `grit/src/commands/clone.rs`

- Ensured local source repository open path goes through `Repository::discover(Some(path))` first.
- Kept local-source safe-directory enforcement helper for clone source access under simulated different-owner mode.

### `grit/src/main.rs`

- Removed redundant top-level `enforce_safe_directory_policy()` call that caused early unscoped rejection formatting.
- Safe-directory enforcement remains rooted in repository discovery/open paths, producing expected command-level error wrapping.

## Validation

### Focus test

- `rm -rf /workspace/tests/trash.t0033-safe-directory /workspace/tests/bin.t0033-safe-directory && cargo build --release -p grit-rs && GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash t0033-safe-directory.sh`
- Result: **22/22 passing**

### Harness confirmation

- `./scripts/run-tests.sh t0033-safe-directory.sh`
- Result: **22/22 passing**

### Regressions

- `./scripts/run-tests.sh t0035-safe-bare-repository.sh` → **12/12 passing**
- `./scripts/run-tests.sh t5601-clone.sh` → **0/0** (existing harness timeout/skip shape unchanged)
- `./scripts/run-tests.sh t5603-clone-dirname.sh` → **0/0** (existing harness timeout/skip shape unchanged)

### Quality gates

- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅
- `cargo test -p grit-lib --lib` ✅ (98/98)

## Outcome

- `t0033-safe-directory` is now fully passing at **22/22**.
- Plan/progress/test-results updated to reflect completion.
