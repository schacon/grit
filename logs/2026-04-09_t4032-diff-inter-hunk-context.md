# t4032-diff-inter-hunk-context

- **Issue:** `diff --inter-hunk-context` and `diff.interHunkContext` were ignored; unified hunks did not fuse like Git/xdiff.
- **Fix:** In `grit-lib::diff`, build hunks with `similar::group_diff_ops` using radius `2 * context + inter_hunk_context`, then render each group with `UnifiedDiffHunk` (same context radius as before). Extended `unified_diff_with_prefix` with `inter_hunk_context: usize` (default `0` via `unified_diff`).
- **`grit diff`:** Resolve inter-hunk context from `--inter-hunk-context` or config `diff.interhunkcontext`; validate invalid/non-integer/negative values on diff entry when CLI flag not set (matches test expectations).
- **Test:** `./scripts/run-tests.sh t4032-diff-inter-hunk-context.sh` → 37/37.
