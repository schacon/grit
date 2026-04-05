# t4073-diff-stat-name-width

- Claimed the `t4073-diff-stat-name-width` task from `plan.md` at 4/6 passing and reproduced the upstream failures with `CARGO_TARGET_DIR=/tmp/grit-build-t4073 bash scripts/run-upstream-tests.sh t4073-diff-stat-name-width 2>&1 | tail -40`.
- Read `git/t/t4073-diff-stat-name-width.sh` and `git/Documentation/diff-options.adoc` to confirm the expected `--stat-name-width` behavior for UTF-8 paths.
- Traced the active failure to `grit/src/commands/diff.rs`: the trailing-arg reparsing path treated `--stat-name-width=...` as plain `--stat`, so the width value was discarded before formatting.
- Fixed trailing `--stat-name-width`, `--stat-width`, `--stat-count`, and `--stat-graph-width` reparsing, and extracted UTF-8-aware diffstat name truncation into `truncate_stat_path()` so padding and suffix selection match the upstream expectations.
- Rebuilt `target/release/grit` with `cargo build --release -p grit-rs` and verified direct output for widths `12` through `1`.
- Confirmed the upstream file now passes completely: `CARGO_TARGET_DIR=/tmp/grit-build-t4073 bash scripts/run-upstream-tests.sh t4073-diff-stat-name-width 2>&1 | tail -40` reported 6/6 passing.
- Ran the requested formatting step: `CARGO_TARGET_DIR=/tmp/grit-build-t4073 cargo fmt --all 2>/dev/null; true`.
