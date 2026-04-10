## 2026-04-10 15:47 UTC — t5531 follow-up delegation attempt (no net gain)

### Goal
Increase `t5531-deep-submodule-push.sh` beyond 18/29 while preserving green push suites.

### Attempted changes

1. `grit/src/commands/push.rs`
   - Added broader delegation gate when the superproject contains `.gitmodules`,
     to route such pushes to system git.

2. `grit/src/transport_passthrough.rs`
   - When delegating, prepended the current executable directory to `PATH` before
     spawning system git so nested `git` invocations inside test wrappers could still
     resolve to the harness-provided shim scripts.

### Validation run

- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5531-deep-submodule-push.sh` → **18/29** (no improvement)
- Verbose rerun still failed in identical cases (`16,18-24,27-29`) with
  `test_must_fail` mismatch and unpopulated-submodule failures.

### Outcome

- Since there was no measurable improvement and the broadened delegation is too
  blunt, both code changes were **reverted**.
- Repository left clean after revert.

### Next focus

- Investigate a narrower native fix for `t5531.16` (check mode stale-branch/remote
  detection when a submodule has commits on multiple branches and one lacks remote),
  and then the unpopulated-submodule path checks (`18-24`, `27-29`).
