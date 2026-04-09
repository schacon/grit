## t4049-diff-stat-count

- Fixed `git diff --stat` / `--stat-count`: mode-only rows (`| 0`), binary line counts, unmerged `Unmerged` row + unique path count in summary, `count_width` from displayed rows only.
- `diff_tree_to_worktree`: staged mode-only vs HEAD, raw-OID normalization, worktree-only exec when index matches tree; `update-index` `--chmod` per-file map from `raw_rest` (not `std::env::args()`).
- `tests/test-lib.sh`: `test_chmod` aligned with upstream (`chmod "$@"` + `git update-index --add "--chmod=$@"`) so `test_chmod +x b d` updates both paths.
- Harness: `./scripts/run-tests.sh t4049-diff-stat-count.sh` → 4/4.
