## Status perf follow-up: t7508 partial dry-run parity

Date: 2026-04-10
Branch: `cursor/grit-status-performance-07bb`

### Scope

Investigated and fixed the `t7508-status.sh` failure:

- `7508.55 dry-run of partial commit excluding new file in index`

### Root cause

`commit --dry-run <pathspec>` used a commit-local untracked scanner that did not honor
status-style ignore behavior and directory collapsing. This diverged from `status` logic and
produced incorrect untracked shapes during partial dry-run status rendering.

Additionally, section spacing in dry-run long output could emit an extra blank line after
branch/tracking headers.

### Changes

- `grit/src/commands/status.rs`
  - Added shared helper:
    - `collect_untracked_normal_for_status(repo, index, work_tree) -> Result<Vec<String>>`
  - This exposes the existing status untracked collector (`IgnoredMode::No`, normal mode)
    for reuse by commit dry-run rendering.

- `grit/src/commands/commit.rs`
  - Replaced the ad-hoc untracked walk with the shared status collector to match
    status-compatible ignore and directory-collapse semantics.
  - Fixed partial dry-run pathspec untracked collapsing:
    - do not collapse a parent directory when that parent includes a pathspec-matched path.
  - Normalized section-spacing behavior in `print_dry_run` to avoid redundant blank lines and
    preserve expected trailing newline behavior.

### Validation

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `bash tests/t7508-status.sh --run=1-55 -v`: test 55 now passes
- `bash tests/t7063-status-untracked-cache.sh -v`: 58/58 pass
- `bash tests/t7519-status-fsmonitor.sh -v`: 33/33 pass
- `bash tests/t7065-status-rename.sh -v`: 28/28 pass
- `./scripts/run-tests.sh t7508-status.sh`: **96/126** (improved from 94/126)
- `./scripts/run-tests.sh t7060-wtstatus.sh`: 12/17 (stable)
- `./scripts/run-tests.sh t7063-status-untracked-cache.sh`: 58/58
- `./scripts/run-tests.sh t7519-status-fsmonitor.sh`: 27/33 (harness-env-known baseline)
- `./scripts/run-tests.sh t7065-status-rename.sh`: 28/28
