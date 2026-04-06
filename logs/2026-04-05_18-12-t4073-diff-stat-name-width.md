## Task: t4073-diff-stat-name-width

### Claim
- Claimed from Diff queue as nearest non-passing target.
- `PLAN.md` marked `t4073-diff-stat-name-width` as `[~]`.

### Baseline
- Current status from `data/file-results.tsv`: `4/6` passing.
- Root-cause hypothesis: trailing argument re-parser in `grit/src/commands/diff.rs` ignores
  `--stat-name-width` (and related stat width/count options) when they appear after revisions.

### Reproduction Notes
- Running `t4073` directly showed width-sensitive assertions failing.
- Manual `grit diff HEAD~1 HEAD --stat --stat-name-width=<n>` output remained untruncated at all widths.

### Next
- Implement trailing-flag parsing for:
  - `--stat-width=<n>`
  - `--stat-name-width=<n>`
  - `--stat-count=<n>`
  - `--stat-graph-width=<n>`
- Rebuild and rerun `t4073`.

### Implementation
- Updated `grit/src/commands/diff.rs` trailing-option re-apply matcher to parse:
  - `--stat-width=<n>`
  - `--stat-name-width=<n>`
  - `--stat-count=<n>`
  - `--stat-graph-width=<n>`
- Each parsed option now sets the corresponding `args.*` field and flips `stat_enabled = true`.
- This ensures stat-width flags work when they appear after revision arguments.

### Validation
- `cargo build --release` ✅
- `bash scripts/run-upstream-tests.sh t4073-diff-stat-name-width` ✅
  - Tests: 6, pass: 6, fail: 0
- `./scripts/run-tests.sh t4073-diff-stat-name-width.sh` ✅
  - 6/6 passing, cache updated in `data/file-results.tsv`
- Hygiene gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated auto-fixes before commit)
  - `cargo test -p grit-lib --lib` ✅

### Completion
- `PLAN.md`: marked `t4073-diff-stat-name-width` as `[x]` with `6/6 (0 left)`.
- `progress.md` and `test-results.md` updated.
