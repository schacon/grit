# t3602-rm-sparse-checkout

## Summary

- Implemented Git-compatible sparse-checkout handling in `grit rm`: `--sparse`, filtering
  index matches by skip-worktree / cone membership, sparse advice (header always; hints
  gated by `advice.updateSparsePath`), and `fatal:` prefix for pathspec errors.
- Fixed non-cone `sparse-checkout set` application to use `path_in_sparse_checkout` semantics
  (`ignore::path_in_sparse_checkout`) instead of sequential `NonConePatterns` toggles.
- `diff_index_to_worktree`: skip `skip_worktree` entries so sparse paths are not shown as
  deleted in `status --porcelain -uno` (matches Git; fixes t3602 recursive rm expectations).
- `status`: omit auto `##` line for porcelain v1 when `-uno` hides untracked (matches Git).
- Extracted `sparse_advice` module shared by `mv` and `rm`.

## Validation

- `./scripts/run-tests.sh t3602-rm-sparse-checkout.sh` — 13/13 pass
- `cargo test -p grit-lib --lib` — pass
