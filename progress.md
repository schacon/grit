# Progress — Grit Test Coverage

**Updated:** 2026-04-05

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    63 |
| In progress |     1 |
| Remaining   |   701 |
| **Total**   |   767 |

## Recently completed

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
- `t4204-patch-id` — 26/26 tests pass (stale plan entry corrected)
- `t4021-format-patch-numbered` — 14/14 tests pass (stale plan entry corrected)
- `t4065-diff-anchored` — 7/7 tests pass (stale plan entry corrected)
- `t4036-format-patch-signer-mime` — 5/5 tests pass (stale plan entry corrected)
- `t4004-diff-rename-symlink` — 4/4 tests pass (stale plan entry corrected)
- `t4005-diff-rename-2` — 4/4 tests pass (stale plan entry corrected)
- `t4043-diff-rename-binary` — 3/3 tests pass (stale plan entry corrected)
- `t4113-apply-ending` — 3/3 tests pass (stale plan entry corrected)
- `t4025-hunk-header` — 2/2 tests pass (stale plan entry corrected)
- `t4066-diff-emit-delay` — 2/2 tests pass (stale plan entry corrected)
- `t4123-apply-shrink` — 2/2 tests pass (stale plan entry corrected)
- `t4134-apply-submodule` — 2/2 tests pass (stale plan entry corrected)
- `t4256-am-format-flowed` — 2/2 tests pass (stale plan entry corrected)
- `t4029-diff-trailing-space` — 1/1 tests pass (stale plan entry corrected)
- `t4110-apply-scan` — 1/1 tests pass (stale plan entry corrected)
- `t4111-apply-subdir` — 10/10 tests pass (stale plan entry corrected)
- `t4028-format-patch-mime-headers` — 3/3 tests pass (stale plan entry corrected)
- `t4062-diff-pickaxe` — 3/3 tests pass (stale plan entry corrected)
- `t4016-diff-quote` — 5/5 tests pass (stale plan entry corrected)
- `t4073-diff-stat-name-width` — 6/6 tests pass (fixed trailing `--stat-*` option parsing after revisions in `grit diff`)
- `t4006-diff-mode` — 7/7 tests pass (binary stat row now renders as `Bin`; update-index `--chmod` now syncs worktree mode to match test helper expectations)
- `t4007-rename-3` — 13/13 tests pass (`diff-files` now honors `-C/--find-copies-harder/-R` and emits copy-raw records for reverse index/worktree diffs)
- `t4125-apply-ws-fuzz` — 4/4 tests pass (`git apply --whitespace=fix` now normalizes context/remove matching and writes whitespace-fixed added lines)

## What Remains

1 test file is currently marked in progress (`t4131-apply-fake-ancestor`) and 701 remain pending. See `plan.md` for the full prioritized list.
