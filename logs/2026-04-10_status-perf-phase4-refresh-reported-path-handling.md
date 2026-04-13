## Scope

Refine `update-index --refresh` fsmonitor semantics so refresh handling is closer to Git for reported-vs-nonreported paths:

- nonreported paths that are already fsmonitor-valid are skipped during refresh;
- reported paths are matched using parent/child path relationships (not just exact byte equality);
- for reported dirty paths, keep fsmonitor-valid state stable during refresh so only reported paths are treated as candidates on subsequent refresh passes in this scenario.

## Code changes

- `grit/src/commands/update_index.rs`
  - Added `fsmonitor_path_matches_reported(path, reported)` helper.
  - In `refresh_index()`:
    - switched reported-path checks from exact `contains()` to path-aware matching.
    - preserved skip behavior only for fsmonitor-valid + not-reported entries.
    - tracked whether a path was fsmonitor-reported (`fsmonitor_considered`).
    - for reported dirty content, return `needs update` without forcing fsmonitor-valid off in this path.

## Validation

- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo build --release -p grit-rs` ✅
- `bash tests/t7519-status-fsmonitor.sh -v`:
  - before this increment: 7 failures (26/33)
  - after this increment: 6 failures (27/33)
  - specifically fixed: test `7519.12 all unmodified files get marked valid`
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh` ✅ `22/33`
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh` ✅ `14/58` (no regression)
- `./scripts/run-tests.sh t7508-status.sh` ✅ `94/126` (no regression)
- `./scripts/run-tests.sh t7060-wtstatus.sh` ✅ `12/17` (no regression)
- `./scripts/run-tests.sh t7065-status-rename.sh` ✅ `28/28` (no regression)
