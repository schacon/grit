## t5533-push-cas — force safety completion

### Scope
- Implement phase-2 push safety behavior for CAS workflows:
  - `--force-if-includes`
  - `push.useForceIfIncludes`
  - correct interaction with `--force-with-lease` forms (including `<ref>:<expect>`)

### Code changes
- `grit/src/commands/push.rs`
  - Added CLI flags:
    - `--force-if-includes`
    - `--no-force-if-includes`
  - Added config support:
    - `push.useForceIfIncludes`
  - Implemented force-if-includes enablement logic with explicit-disable precedence.
  - Reworked force-with-lease evaluation per remote ref:
    - parse and match bare/ref/ref:expect modes
    - handle per-ref targeting correctly
    - support missing-tracking behavior for creation/delete cases
  - Implemented includes checks using both:
    - tracking-tip ancestry
    - reflog reachability fallback (for rebased/rewritten histories)
  - Enforced explicit `<ref>:<expect>` disables includes behavior as required.
  - Kept `--force` / `+<refspec>` semantics overriding lease rejections where appropriate.
  - Fixed push result formatting to emit `forced update` markers in CAS rewrite cases.
- `grit/src/commands/pack_objects.rs`
  - Kept previously validated MIDX-absence fallback needed by this suite setup path.

### Validation commands
- `cargo build --release -p grit-rs`
- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo clippy --fix --allow-dirty -p grit-rs` (reverted unrelated formatting-only files)
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t5533-push-cas.sh` (re-run twice for stability)

### Result
- `t5533-push-cas.sh`: **23/23 passed**
