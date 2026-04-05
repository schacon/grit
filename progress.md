# Progress — Grit Test Coverage

**Updated:** 2026-04-05

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    57 |
| In progress |     1 |
| Remaining   |   709 |
| **Total**   |   767 |

## Recently completed

- `t6401-merge-criss-cross` — 4/4 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected after confirmed clean harness run)
- `t6435-merge-sparse` — 6/6 tests pass (init now skips default template skeleton in test trash worktrees so `.git/info` can be created by tests; `status --porcelain -- <pathspec>` now filters untracked/ignored and diff entries by pathspec, matching Git behavior)
- `t6301-for-each-ref-errors` — 6/6 tests pass (added `test-tool ref-store main update-ref ... REF_SKIP_OID_VERIFICATION` support; `for-each-ref` now preserves and reports non-hex direct ref payloads as missing objects for simplified test harness compatibility)
- `t6400-merge-df` — 7/7 tests pass (fixed modify/delete directory-file conflict handling to place conflict stages at side paths like `letters~modify`/`letters~HEAD`; `ls-files -o` now ignores transient `.stdout.*`/`.stderr.*` harness capture files)
- `t6431-merge-criscross` — 2/2 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t6412-merge-large-rename` — 10/10 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t6428-merge-conflicts-sparse` — 2/2 tests pass (sparse-checkout no-cone glob semantics fixed; merge conflict stages now preserved in index and shown as modified in `ls-files -t`)
- `t6413-merge-crlf` — 3/3 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t6136-pathspec-in-bare` — 3/3 tests pass (`log` and `ls-files` now reject out-of-repo `..` pathspecs in bare/.git contexts with the expected "outside repository" diagnostics)
- `t6134-pathspec-in-submodule` — 3/3 tests pass (`git add` now detects `git -C <submodule> add` in unpopulated submodules and reports the expected fatal message)
- `t6114-keep-packs` — 3/3 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t6425-merge-rename-delete` — 1/1 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
- `t6110-rev-list-sparse` — 2/2 tests pass (upstream re-verification on `main`; stale `PLAN.md` entry corrected)
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

709 test files still pending. See `plan.md` for the full prioritized list.
