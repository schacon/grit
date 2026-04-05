# Progress — Grit Test Coverage

**Updated:** 2026-04-05

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    70 |
| In progress |     0 |
| Remaining   |   696 |
| **Total**   |   766 |

## Recently completed

- `t2202-add-addremove` — 3/3 tests pass (`git --literal-pathspecs add --all` now works because the top-level parser accepts Git's pathspec-mode global flags before dispatch, and the upstream `t2202` rerun completed green against the refreshed release binary)
- `t3450-history` — 2/2 tests pass (re-ran the requested upstream verification on `main`; `target/release/grit` already matched the two placeholder `git history` failure cases, so the `PLAN.md` entry was stale and no Rust code changes were required)
- `t3307-notes-man` — 3/3 tests pass (re-ran the requested upstream verification on `main`; `target/release/grit` already matched the manpage examples for text notes and binary notes, so the remaining `PLAN.md` entry was stale and no Rust code changes were required)
- `t3423-rebase-reword` — 3/3 tests pass (`rebase -i` now supports the scoped `pick`/`reword` todo flow, reworded commits reopen the commit-message editor after conflicts on `rebase --continue`, and `checkout --theirs <path>` now restores stage 3 content during conflict resolution)
- `t3702-add-edit` — 3/3 tests pass (`git add -e` now opens the index-vs-worktree patch in the configured shell-style editor, applies the edited result back to the index with hunk recounting, preserves the working tree, and fails cleanly when the editor exits non-zero)
- `t3012-ls-files-dedup` — 3/3 tests pass (re-ran the requested upstream verification on `main`; `target/release/grit` already handled `git ls-files --deduplicate` correctly, so the `PLAN.md` entry was stale and no Rust code changes were required)
- `t3008-ls-files-lazy-init-name-hash` — 1/1 tests pass (the source tree already had the required `test-tool online-cpus` and `test-tool lazy-init-name-hash` support; rerunning upstream verification against the current `target/release/grit` confirmed this was a stale `PLAN.md` entry rather than a remaining Rust code gap)
- `t2023-checkout-m` — 5/5 tests pass (`checkout` now persists resolve-undo data in the index, `checkout -m -- <path>` recreates conflicted files from saved stages, and `checkout -m <branch>` restores conflict stages when a branch switch hits local content conflicts)
- `t3908-stash-in-worktree` — 2/2 tests pass (re-ran the requested upstream verification on `main`; `target/release/grit` already handled `git stash` from a subdirectory in a linked worktree correctly, so the `PLAN.md` entry was stale and no Rust code changes were required)
- `t1512-rev-parse-disambiguation` — 3/3 tests pass (`rev-parse` now emits Git-style short-object ambiguity diagnostics for ambiguous blobs, invalid loose objects, and corrupt loose objects; the tracked 3-test slice was re-verified directly with the missing hash helpers loaded from `tests/test-lib-functions.sh`)
- `t3009-ls-files-others-nonsubmodule` — 2/2 tests pass (re-ran the requested upstream verification on `main`; `target/release/grit` already handled nested non-submodule repositories correctly, so the `PLAN.md` entry was stale and no Rust code changes were required)
- `t3502-cherry-pick-merge` — 12/12 tests pass (the requested upstream verification passed after clearing a stale `/tmp/grit-upstream-workdir`; the `PLAN.md` entry was stale and no Rust code changes were required)
- `t3302-notes-index-expensive` — 12/12 tests pass (re-ran the requested upstream verification on `main`; this checkout already had the fix, so the stale `PLAN.md` entry was corrected and no Rust code changes were required)
- `t2104-update-index-skip-worktree` — 7/7 tests pass (re-ran the requested upstream verification on `main`; the `PLAN.md` entry was stale and no Rust code changes were required)
- `t2010-checkout-ambiguous` — 10/10 tests pass (re-ran the requested upstream verification on `main`; `plan.md` was already marked complete and no Rust code changes were required)
- `t2012-checkout-last` — 22/22 tests pass (upstream re-verification on `main`; the `plan.md` entry was stale and no Rust code changes were required)
- `t2050-git-dir-relative` — 4/4 tests pass (upstream re-verification on `main`; the `PLAN.md` entry was stale and no Rust code changes were required)
- `t1015-read-index-unmerged` — 6/6 tests pass (`merge --abort` and `am --skip` now survive directory/file conflicts, and `format-patch -1 <rev>` now formats the requested single commit)
- `t2105-update-index-gitfile` — 4/4 tests pass (upstream re-verification on `main`; `update-index` already resolves `.git` directories and gitfiles to stage a `160000` gitlink, so the stale `PLAN.md` entry was corrected)
- `t2015-checkout-unborn` — 6/6 tests pass (upstream re-verification on rebuilt `target/release/grit`; the `PLAN.md` entry was stale and no Rust code changes were required)
- `t1412-reflog-loop` — 3/3 tests pass (`checkout -b` now writes the new branch's creation reflog entry, so `git log -g topic` includes `topic@{4} branch: Created from HEAD`)
- `t1600-index` — 7/7 tests pass (index writes now honor `index.skipHash`, `feature.manyFiles`, and on-disk v4 serialization; `test-tool hexdump` support was added for upstream helper usage)
- `t1407-worktree-ref-store` — 4/4 tests pass (`test-tool ref-store` now supports worktree stores for `resolve-ref` and `create-symref`, and the upstream runner's fake `test-tool` wrapper now preserves its argv)
- `t1601-index-bogus` — 4/4 tests pass (upstream re-verification on `main`; the `plan.md` entry was stale and no Rust code changes were required)
- `t1503-rev-parse-verify` — 12/12 tests pass (`reflog delete --rewrite` is accepted and date-based reflog verification now resolves selectors like `1.year.ago`)
- `t1408-packed-refs` — 3/3 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected after rebuilding `target/release/grit`)
- `t3102-ls-tree-wildcards` — 4/4 tests pass (`ls-tree` and `ls-files` now agree on negated pathspec handling for wildcard filters)
- `t3205-branch-color` — 4/4 tests pass (upstream re-verification on `main`; stale `plan.md` entry corrected after rebuilding `target/release/grit`)
- `t3500-cherry` — 4/4 tests pass (upstream re-verification on `main`; stale `plan.md` entry corrected)
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
- `t1303-wacky-config` — 11/11 tests pass (requested upstream rerun confirmed the file is already green; the stale duplicate `plan.md` entry was removed)
- `t1402-check-ref-format` — 99/99 tests passing (was 97/99)
- `t1505-rev-parse-last` — 7/7 tests pass (@{-N} syntax fully working)
- `t1100-commit-tree-options` — 5/5 tests pass
- `t1418-reflog-exists` — 6/6 tests pass
- `t0101-at-syntax` — 8/8 tests pass (`@{...}` reflog syntax cases validated)

## What Remains

696 test files still pending. See `PLAN.md` for the full prioritized list.
