## 2026-04-10 — update-index split-index option compatibility (fsmonitor suite support)

### Scope

- Continue Phase 4 fsmonitor parity work by accepting split-index CLI switches used by
  status-associated harness tests.

### Code changes

1. `grit/src/commands/update_index.rs`
   - Added support for parsing:
     - `--split-index`
     - `--no-split-index`
   - Added conflict validation for mutually exclusive use:
     - `--split-index` + `--no-split-index` now errors.
   - Current behavior is compatibility no-op (option accepted, no split-index backend semantics yet).

### Validation

- `cargo fmt`
- `cargo check -p grit-rs`
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh` → 12/33

Result: option-parsing failure on `--split-index` in t7519 is removed; remaining failures are deeper
fsmonitor/status parity and harness setup side effects.
