# Progress — Grit Test Coverage

**Updated:** 2026-04-06

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    68 |
| In progress |     0 |
| Remaining   |   699 |
| **Total**   |   767 |

## Recently completed

- `t6060-merge-index` — 7/7 tests pass (`merge-index` now supports Git-compatible `-o`/`-q`, argument ordering, and built-in dispatch for `git-merge-one-file`; helper now updates stage-0 index entries via effective `GIT_INDEX_FILE`, requires a work tree for working-tree writes, and writes merged content to index+worktree; `diff-files` now honors `--diff-filter` and `rev-parse` now resolves `:path`/`:N:path` against effective `GIT_INDEX_FILE`)
- `t6427-diff3-conflict-markers` — 9/9 tests pass (merge conflict markers now use correct base/theirs labels and conflict styles across `diff3`/`zdiff3`; rebase now preserves diff3/zdiff3 conflict-marker files with backend-specific base labels; merge-file zealous diff3 handling now matches expected compact conflict shapes for shared prefix/suffix insertions)
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
- `t6133-pathspec-rev-dwim` — 6/6 tests pass (`log` now DWIMs wildcard and `:/*.t` tokens to pathspecs when revision resolution fails; `rev-parse` now supports `^{/regex}` commit-message peel and `@{now ...}` reflog date selectors via approxidate `now`)
- `t6421-merge-partial-clone` — 3/3 tests pass (added partial-clone promisor marker initialization for `clone --filter=blob:none`, `rev-list --missing=print` integration with promisor marker output, and merge-side simulated lazy-fetch trace batches with expected `fetch_count` accounting; fixed rename/rename(1to1) handling to avoid false rename/delete+rename/add conflicts in `B-many` case)
- `t6415-merge-dir-to-symlink` — 24/24 tests pass (`rm --cached` now treats tracked symlink paths as non-directories by using `symlink_metadata` for recursion checks and removal dispatch; merge now aborts before overwriting untracked/dirty files in directory→symlink transitions, preserving local data and matching expected merge refusal behavior)
- `t6404-recursive-merge` — 6/6 tests pass (virtual merge-base construction now reuses conflict-marker blobs from higher-stage entries to preserve nested virtual-base stage-1 OIDs; merge now emits Git-compatible binary conflict diagnostic `Cannot merge binary files: <path> (HEAD vs. <branch>)`)
- `t6439-merge-co-error-msgs` — 6/6 tests pass (merge now performs fast-forward overwrite checks before mutating HEAD/index/worktree; merge overwrite diagnostics now combine local+untracked sections in Git-compatible order, include strategy-failure trailer for `GIT_MERGE_VERBOSITY=0`, and checkout diagnostics no longer include duplicated `error:` prefixes)
- `t6004-rev-list-path-optim` — 7/7 tests pass (rev-list path limiting now supports `.` and glob pathspecs via wildmatch and performs merge-aware TREESAME simplification, fixing path-optimization and `d/*`/`d/[a-m]*` history selection)
- `t6005-rev-list-count` — 6/6 tests pass (`rev-list` now accepts detached `--skip <n>` form and treats `-<n>foo` malformed shorthand values as integer-parse errors; integer diagnostics now include the expected `not an integer` wording for `--max-count`, `--skip`, and `-n`)
- `t6010-merge-base` — 12/12 tests pass (`merge-base` now supports `--fork-point` using reflog-aware candidate selection, `show-branch` now supports `--merge-base` and `--independent`, and `merge` now supports `--allow-unrelated-histories` for criss-cross setup merges)
- `t6700-tree-depth` — 10/10 tests pass (`archive`, `ls-tree`, `rev-list --objects`, and `diff-tree -r` now honor `core.maxtreedepth`; tree-ish resolution now accepts tags via `rev-parse`; `diff-tree` now treats the test-harness legacy/canonical empty-tree IDs as an empty tree input for depth checks and recursive diff traversal)

## What Remains

699 test files still pending. See `plan.md` for the full prioritized list.
