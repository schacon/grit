## 2026-04-10 16:27 UTC — Preserve populated gitlink worktrees on checkout

### Goal
Fix remaining deep submodule push failures in `t5531-deep-submodule-push.sh` caused by submodule
worktrees becoming unpopulated after branch checkout in the superproject.

### Root cause
- During superproject branch switches, checkout code removed directories for dropped gitlink
  entries too aggressively.
- For classic embedded submodule-style setups used by `t5531` (gitlink entries without
  `.git/modules/...` administration), switching branches could remove `work/gar/bage/.git`,
  leaving the submodule unpopulated.
- Subsequent recursive push checks then failed with:
  - `fatal: in unpopulated submodule 'gar/bage'`
  - or missed remote-tracking checks that depend on a populated nested repo.

### Code change
- Updated `grit/src/commands/checkout.rs` in `checkout_index_to_worktree`:
  - For stage-0 paths removed from the index, keep existing behavior of **not** deleting gitlink
    directories in superproject checkouts.
  - Restrict forced deletion of dropped gitlink directories (`force_remove_populated_submodule`)
    to **nested modules repos only** (`git_dir_is_nested_modules_repo(&repo.git_dir)`), where
    cleanup is required for internal module layout transitions.
  - This preserves populated submodule worktrees for superproject branch switches, matching Git’s
    behavior for the `t5531` matrix.

### Validation
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5531-deep-submodule-push.sh` → **29/29** ✅
- Regression checks:
  - `./scripts/run-tests.sh t5517-push-mirror.sh` → 13/13 ✅
  - `./scripts/run-tests.sh t5538-push-shallow.sh` → 8/8 ✅
  - `./scripts/run-tests.sh t5545-push-options.sh` → 13/13 ✅
  - `./scripts/run-tests.sh t5509-fetch-push-namespaces.sh` → 13/15 (unchanged baseline) ✅

### Notes
- `t5509` remains at 13/15 with two known failures that reproduce under system git in this local
  harness (`transfer.hideRefs` namespace-stripping semantics), so this slice focused on `t5531`.
