# Progress — Grit Test Coverage

**Updated:** 2026-04-05

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    57 |
| In progress |     0 |
| Remaining   |   710 |
| **Total**   |   767 |

## Recently completed

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

710 test files still pending. See `plan.md` for the full prioritized list.
