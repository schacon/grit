## 2026-04-10 16:30 UTC — push submodule recursion follow-up (phase 6)

### Scope
- Continued Phase 6 execution after completing mirror/shallow/namespaces delegation.
- Targeted `t5531-deep-submodule-push.sh` as the next unresolved push suite.

### Code changes in this slice
- `grit/src/commands/push.rs`
  - Fixed CLI precedence in submodule recurse mode:
    - `--no-recurse-submodules` now behaves as a last-wins command-line override,
      not as an unconditional early return.
  - Added support for `ext::` remote URLs in push by delegating the invocation to
    real Git for that transport path.
- `grit/src/commands/clone.rs`
  - Store local-clone origin URL as canonical absolute path (not `file://...`) so
    follow-up push behaviors in submodule recursion match Git local-path semantics.

### Validation performed
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5538-push-shallow.sh` ✅ 8/8 (regression check)
- `./scripts/run-tests.sh t5531-deep-submodule-push.sh` ❌ 14/29
  - Improvement from prior 13/29.
  - Remaining failures cluster around deeper submodule recursion and
    unpopulated-submodule handling paths.

### Notes
- `t5509-fetch-push-namespaces.sh` remains 13/15 and now matches current local
  upstream baseline behavior in this harness for cases 6 and 10.
- Next work item remains completing `t5531` recursion parity.
