## t1407-worktree-ref-store

- Claimed from plan at 1/4.
- Reproduced with:
  - `./scripts/run-tests.sh t1407-worktree-ref-store.sh`
  - `GUST_BIN=/workspace/target/release/grit TEST_VERBOSE=1 bash tests/t1407-worktree-ref-store.sh`

### Failure analysis

- Tests failed because `test-tool ref-store` was missing entirely:
  - `error: test-tool: unknown subcommand 'ref-store'`

### Implementation

- Updated `grit/src/main.rs`:
  - added `run_test_tool_ref_store()` and dispatch registration under `test-tool`;
  - added worktree selector resolver for `worktree:<id>` (`main` and linked worktrees under `.git/worktrees/<id>`);
  - implemented `resolve-ref <ref> <flags>` output shape expected by tests:
    - shared refs: `<oid> <refname> 0x0`
    - per-worktree HEAD resolution: `<oid> <symbolic-target> 0x1`
  - implemented `create-symref <name> <target> <logmsg>` for worktree-specific symbolic refs.

### Validation

- `cargo fmt && cargo build --release -p grit-rs` ✅
- copied release binary to test harness path (`cp target/release/grit tests/grit`) ✅
- `./scripts/run-tests.sh t1407-worktree-ref-store.sh` ✅ (4/4)
- `GUST_BIN=/workspace/tests/grit TEST_VERBOSE=1 bash tests/t1407-worktree-ref-store.sh` ✅ (4/4)

### Tracking updates

- `PLAN.md`: `t1407-worktree-ref-store` marked complete (4/4).
- `progress.md`: counts refreshed.
- `test-results.md`: appended test/build evidence.
