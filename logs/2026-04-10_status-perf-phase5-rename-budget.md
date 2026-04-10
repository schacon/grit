## 2026-04-10 — status rename/copy budget guardrails (Phase 5)

### Scope

- Implement Phase 5 budget controls for status rename detection without changing
  default behavior on normal-sized change sets.
- Keep compatibility with existing status output while preventing expensive
  delete×add candidate explosions during large refactors.

### Code changes

1. `grit/src/commands/status.rs`
   - Added `detect_renames_for_status(...)` wrapper around `detect_renames(...)`.
   - The wrapper counts Added and Deleted candidates and skips rename pairing
     when either of these limits is exceeded:
     - total add+delete candidates > `2000`
     - matrix size (`deleted * added`) > `50_000`
   - Updated staged and unstaged rename paths in `status` to call this wrapper.

### Why this matches the plan

- The plan called for “bound candidate pairing work” and “keep behavior parity
  defaults.”
- This implementation preserves existing rename detection for typical repos and
  only avoids high-cost matrix work in pathological large-change cases.

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `./scripts/run-tests.sh t7065-status-rename.sh` → **27/28**
- `./scripts/run-tests.sh t7508-status.sh` → **48/126**

No new regressions were observed in these targeted status suites versus the
current baseline.
