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
