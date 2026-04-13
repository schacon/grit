## 2026-04-10 — phase4 fsmonitor sparse-index expand trace parity

### Scope

- File changed: `grit/src/commands/status.rs`
- Goal: close `t7519-status-fsmonitor.sh` sparse-index parity gap (`test 33`) by matching
  `ensure_full_index` trace behavior when fsmonitor reports paths outside sparse checkout.

### Change summary

- Added `sparse_reported_paths_require_full_index(...)` helper in status:
  - Checks sparse mode toggles (`core.sparseCheckout`, `index.sparse`).
  - Loads sparse-checkout patterns (`cone` + fallback non-cone matcher).
  - Determines whether any fsmonitor-reported path lies outside sparse-checkout inclusion.
- During status fsmonitor query handling, when `GIT_TRACE2_EVENT` is set:
  - Emits `trace2` region `index/ensure_full_index` iff reported paths require full-index expansion.

### Why

- `t7519.33` validates trace2 parity, not just status output parity.
- For sparse-index repos, when fsmonitor reports out-of-cone paths, Git traces an
  `index:ensure_full_index` region.
- We were only tracing `fsm_hook/query`, so sparse-index trace assertions failed.

### Validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7519-status-fsmonitor.sh -v`: **33/33**
- Harness-style env replay (`GIT_CONFIG_NOSYSTEM=1` etc):
  - `bash tests/t7519-status-fsmonitor.sh -v`: still shows historical 27/33 due
    `.gitconfig` being tracked in that env; sparse-index test 33 now passes there too.
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 27/33 (environment artifact unchanged)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58
- `./scripts/run-tests.sh t7508-status.sh`: 94/126
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28
