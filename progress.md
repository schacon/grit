# Progress — Grit Test Coverage

**Updated:** 2026-04-06

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    77 |
| In progress |     0 |
| Remaining   |   690 |
| **Total**   |   767 |

## Recently completed

- `t1060-object-corruption` — 17/17 tests pass (hardened local clone object-copy validation to fail on corrupt/missing/misnamed objects before bare clone completion, and taught `rev-list --objects` to tolerate the canonical empty tree object as implicitly present when missing from loose storage)
- `t1302-repo-version` — 18/18 tests pass (added repository format/extension validation for supported v0/v1 repos and expected extension semantics, enforced local config operations against unsupported repos, blocked `apply --check --index` on invalid repositories, and made `prune` refuse precious-objects repositories while preserving existing `repack -ad` protections)
- `t1090-sparse-checkout-scope` — 7/7 tests pass (implemented sparse-checkout scope compatibility across checkout/merge/fetch paths: added `checkout-index --ignore-skip-worktree-bits`, `clone --template` acceptance for sparse fixture setup, `config -C <path>` compatibility/no-op behavior, `fetch --filter=blob:none` sparse-aware blob retention, and `rev-list --missing=print` support used by partial sparse validation)
- `t0411-clone-from-partial` — 7/7 tests pass (implemented blob:none partial-clone shaping for local clone fixtures, disabled clone-time lazy fetch by default with `lazy fetching disabled` diagnostics, and added promisor lazy-fetch attempt for `pack-objects --revs` via configured `remote.origin.uploadpack`)
- `t1012-read-tree-df` — 5/5 tests pass (fixed `read-tree -m` 3-way merge to preserve and advance existing unmerged stages from the current index so D/F conflict cases maintain expected stage-1/2/3 entries without collapsing to stage-0)
- `t1512-rev-parse-disambiguation` — 3/3 tests pass (implemented rev-parse ambiguous-short-id diagnostics with candidate hints/bad-object handling, added `test-tool` hash/zlib compatibility helpers, and restored missing test-lib helper functions used by loose-object fixtures)
- `t1051-large-conversion` — 12/12 tests pass (fixed checkout path mode to treat `<paths...>` as pathspecs when no `--` is provided and the first token is not a commit-ish, preserving ambiguity diagnostics for real commit-ish/path collisions)
- `t1412-reflog-loop` — 3/3 tests pass (restored branch-creation reflog entries for checkout-created branches and fixed append semantics in test helper commits so reflog walk history matches upstream)
- `t2006-checkout-index-basic` — 9/9 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t1407-worktree-ref-store` — 4/4 tests pass (implemented `test-tool ref-store` worktree backend for `resolve-ref` and `create-symref` operations used by upstream API coverage)
- `t1901-repo-structure` — 4/4 tests pass (implemented `git repo structure` output/progress compatibility for empty repositories and progress-meter behavior used by upstream tests)
- `t2018-checkout-branch` — 25/25 tests pass (fixed checkout compatibility for `@{-1}` branch names, clone `--no-checkout` population behavior, sparse-checkout branch creation, and canonical branch/path argument errors)
- `t2202-add-addremove` — 3/3 tests pass (added global `--literal-pathspecs` handling so `git add --all` setup and no-op semantics match upstream tests)
- `t2027-checkout-track` — 5/5 tests pass (added checkout/switch ambiguous remote-tracking branch hints including `git switch --track` guidance)
- `t2023-checkout-m` — 5/5 tests pass (implemented checkout `-m` conflict restoration for both path mode and branch-switch mode, including correct stage-2/stage-3 ordering)
- `t2104-update-index-skip-worktree` — 7/7 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t2010-checkout-ambiguous` — 10/10 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t1600-index` — 7/7 tests pass (implemented index v4 write/read support with path compression + `index.skipHash`/`feature.manyFiles` trailing-hash behavior, and added missing `test_trailing_hash` helper command in `tests/`)
- `t2012-checkout-last` — 22/22 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t2105-update-index-gitfile` — 4/4 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t2015-checkout-unborn` — 6/6 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t2050-git-dir-relative` — 4/4 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t1503-rev-parse-verify` — 12/12 tests pass (added `reflog delete --rewrite` compatibility and improved reflog approxidate fallback for date selectors like `1.year.ago`)
- `t1015-read-index-unmerged` — 6/6 tests pass (fixed D/F conflict cleanup in `merge --abort`, `am --skip`, and `format-patch -1 <rev>` target selection)
- `t1408-packed-refs` — 3/3 tests pass (upstream re-verification on current branch; stale `PLAN.md` entry corrected)
- `t0070-fundamental` — 11/11 tests pass (implemented missing `test-tool` helpers in `grit` and fixed `tests/test-tool` pkt-line delegation)
- `t3307-notes-man` — 3/3 tests pass (restored missing upstream binary fixtures `test-binary-1.png` and `test-binary-2.png` in `tests/`)
- `t1601-index-bogus` — 4/4 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3012-ls-files-dedup` — 3/3 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3205-branch-color` — 4/4 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3008-ls-files-lazy-init-name-hash` — 1/1 tests pass (implemented missing `test-tool online-cpus` and `test-tool lazy-init-name-hash` subcommands in grit)
- `t3908-stash-in-worktree` — 2/2 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3009-ls-files-others-nonsubmodule` — 2/2 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3500-cherry` — 4/4 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3102-ls-tree-wildcards` — 4/4 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t0050-filesystem` — 13/13 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3004-ls-files-basic` — 6/6 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3304-notes-mixed` — 6/6 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t3211-peel-ref` — 8/8 tests pass (upstream re-verification on `main`; stale `plan.md` entry corrected)
- `t3003-ls-files-exclude` — 7/7 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t1311-config-optional` — 3/3 tests pass (`:(optional)` config-path handling was already implemented; upstream re-verification showed the plan entry was stale)
- `t0009-git-dir-validation` — 6/6 tests pass (upstream verification on `main`; stale conflicted `plan.md` entry corrected)
- `t1003-read-tree-prefix` — 3/3 tests pass (`read-tree --prefix` no longer writes a bogus v4 index under `GIT_INDEX_VERSION=4`)
- `t1401-symbolic-ref` — 25/25 tests pass (upstream harness now exports `TAR`, so the setup/reset cases run correctly)
- `t1307-config-blob` — 13/13 tests pass (`git config --blob` handling re-verified against upstream; stale plan entry corrected)
- `t0213-trace2-ancestry` — 5/5 tests pass (`cmd_ancestry` trace2 coverage complete)
- `t1310-config-default` — 5/5 tests pass (`git config --default` validation and typed fallback handling)
- `t2060-switch` — 16/16 tests pass
- `t1303-wacky-config` — 11/11 tests pass (stale plan entry corrected after upstream verification)
- `t1402-check-ref-format` — 99/99 tests passing (was 97/99)
- `t1505-rev-parse-last` — 7/7 tests pass (@{-N} syntax fully working)
- `t1100-commit-tree-options` — 5/5 tests pass
- `t1418-reflog-exists` — 6/6 tests pass
- `t0101-at-syntax` — 8/8 tests pass (`@{...}` reflog syntax cases validated)

## What Remains

690 test files still pending. See `plan.md` for the full prioritized list.
