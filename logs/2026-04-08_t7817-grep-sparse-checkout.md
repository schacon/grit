# t7817-grep-sparse-checkout

## Goal

Make `tests/t7817-grep-sparse-checkout.sh` pass 8/8 with grit.

## Changes (summary)

- **grit-lib `ignore.rs`**: Added `path_in_sparse_checkout` — Git-style non-cone sparse evaluation (walk parents, last matching pattern wins, directory-only patterns only when matching as directory). Wired `sparse_checkout::path_matches_sparse_patterns` non-cone branch to it.
- **grit `sparse_checkout.rs`**: `init` preserves `core.sparseCheckoutCone` after `disable`; when `info/sparse-checkout` is missing, write `/*` + `!/*/` before apply (not lone `/*`). `disable` no longer deletes the sparse-checkout file so later `init` reapplies stored patterns (needed for t7817 flow).
- **grit `submodule.rs`**: After submodule checkout, reapply sparse if enabled; after `reset --hard` in `submodule update`, reapply sparse again (reset was repopulating cone submodule trees).
- **grit `grep.rs`**: Worktree mode: skip `SKIP_WORKTREE` only when path missing; `CE_VALID` uses index unless `SKIP_WORKTREE` (then worktree path). Unmerged paths: grep worktree file once. `--cached`: grep each conflict stage blob; dedupe stage-0 paths only for normal entries.

## Validation

- `./scripts/run-tests.sh t7817-grep-sparse-checkout.sh` → 8/8
- `cargo test -p grit-lib --lib`

## Git

Branch: `cursor/t7817-grep-sparse-checkout-fa1d`
