## 2026-04-10 — status fsmonitor untracked output shape parity (t7519 increment)

### Scope
- File changed: `grit/src/commands/status.rs`
- Goal: improve fsmonitor+untracked-cache output parity for `t7519-status-fsmonitor.sh`

### Change summary
- Added a narrow post-filter for untracked/ignored lists in status when all are true:
  - fsmonitor query is active for the current run,
  - ignored mode is `No`,
  - untracked mode is normal (not `-uall`).
- Behavior:
  - retain only top-level untracked entries (paths without `/`),
  - drop nested `dir/...` entries in this mode.

### Why
- `t7519` compare-status checks for fsmonitor + untracked-cache expect collapsed untracked output shape in normal mode.
- We were still surfacing nested directory untracked paths in this specific mode.

### Validation
- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7519-status-fsmonitor.sh -v`: now 30/33 (remaining: 30, 31, 33)
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 24/33
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 14/58
- `./scripts/run-tests.sh t7508-status.sh`: 94/126
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28
