# t3419-rebase-patch-id

- Rebase todo list now uses `rev_list` with symmetric `upstream...HEAD`, `--cherry-pick --right-only`, and topo order (skip patch-id duplicates like Git).
- `rev_list` cherry matching now uses `patch_ids::compute_patch_id`; symmetric left/right uses reachability difference (not intersection of closures).
- `compute_patch_id` aligned with Git `diff_get_patch_id` (per-file carry, mode lines, binary OIDs, `---`/`+++` prefixes).
- Checkout: when skipping blob rewrite for identical content, still apply index executable bit.
- Rebase merge: same OID on base/ours/theirs with mode mismatch takes `theirs` (mode-only cherry-pick).
- `git diff`: pure mode change with same blob omits index line and hunks (matches Git).
- Added `--reapply-cherry-picks` / `--no-reapply-cherry-picks` on rebase; `pull` passes defaults.

Harness: `./scripts/run-tests.sh t3419-rebase-patch-id.sh` → 8/8.
