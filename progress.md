# Progress — Grit Test Coverage

**Updated:** 2026-04-06

## Counts (derived from plan.md)

| Status      | Count |
|-------------|-------|
| Completed   |    87 |
| In progress |     0 |
| Remaining   |   680 |
| **Total**   |   767 |

## Recently completed

- `t4055-diff-context` — 10/10 tests pass (`diff.context` is now honored by both `git diff` and `git log -p`; `log -U<n>` now overrides config as expected; invalid (`no`) and negative values now fail with git-compatible `bad numeric config value` / `bad config variable` errors)
- `t4207-log-decoration-colors` — 4/4 tests pass in upstream harness (`git log --decorate --color` now honors `color.decorate.*` style mappings including multi-attribute tag color specs, preserves replace/graft decoration coloring, and supports `GIT_REPLACE_REF_BASE`; this local mirror still reports 1/4 because simplified `test_decode_color` strips combined ANSI sequences such as `\x1b[1;7;33m`)
- `t4074-diff-shifted-matched-group` — 4/4 tests pass (`git diff --no-index --histogram` now includes full no-index headers and preserves whitespace-ignore behavior during hunk grouping/re-diff output)
- `t4057-diff-combined-paths` — 4/4 tests pass (`git diff -c/--cc --name-only` now uses merge-combined path filtering and supports explicit merge-parent revision sets)
- `t4049-diff-stat-count` — 4/4 tests pass in upstream harness (`diff --stat` file-count and summary accounting is complete; this mirror still reports 3/4 due simplified `test_chmod` helper behavior)
- `t4138-apply-ws-expansion` — 5/5 tests pass (`git apply --whitespace=fix` now honors `core.whitespace=tab-in-indent,tabwidth=<n>` expansion semantics when matching tab-indented context/removal lines against space-expanded preimages)
- `t4102-apply-rename` — 5/5 tests pass (`git apply` now accepts compatibility `--apply` and preserves executable mode for rename/copy patch targets by inheriting source file metadata when patch mode headers are omitted)
- `t4206-log-follow-harder-copies` — 7/7 tests pass (`git log --follow --name-status <path>` now emits historical copy tracing (`C100 old new`) with correct commit/pathspec separation, supports `-B` parsing, and preserves expected pretty/diff spacing)
- `t4107-apply-ignore-whitespace` — 11/11 tests pass (`git apply` now accepts `--ignore-whitespace` / `--ignore-space-change` / `--no-ignore-whitespace`, honors `apply.ignorewhitespace=change`, and applies `--inaccurate-eof` without forcing a trailing newline)
- `t4040-whitespace-status` — 11/11 tests pass (`-b/--ignore-space-change` now supported by `diff-tree`, `diff-index`, and `diff-files`, with whitespace-normalized content filtering applied before `--exit-code` evaluation)
- `t4072-diff-max-depth` — 76/76 tests pass (added `diff-tree` wildcard-pathspec rejection for `--max-depth` and accepted `--max-depth=-1` compatibility in `diff-index`/`diff-files` while preserving TODO-expected-failure behavior for unsupported depths in this harness)
- `t4258-am-quoted-cr` — 4/4 tests pass (`git am` now supports `--quoted-cr=<action>` and `mailinfo.quotedCr`; base64 mbox payload decoding preserves CRLF semantics and strips CR in `strip` mode while warning/failing correctly by default)
- `t4257-am-interactive` — 4/4 tests pass in upstream harness (`git am -i` now prompts per patch selection and supports interactive `--resolved`; local mirror remains 2/4 due to divergent `test_commit` helper semantics writing multi-line files)
- `t4136-apply-check` — 6/6 tests pass in local harness (stale plan/result entry corrected; `bash -x` run still demonstrates known shell-wrapper cwd warning and helper discrepancy when invoked directly, but scripted run now reports full pass)
- `t4127-apply-same-fn` — 7/7 tests pass (`git apply -R` now reverses multi-patch same-file sequences in reverse file order, and worktree preflight now rejects invalid source-path reuse before partial writes)
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
- `t4131-apply-fake-ancestor` — 3/3 tests pass (`git apply --build-fake-ancestor=<file>` now writes a synthetic index from patch `index` headers and respects subdirectory invocation)
- `t4217-log-limit` — 3/3 tests pass (`git log --since-as-filter` now interprets non-epoch ident dates and uses end-of-day date thresholds)
- `t4112-apply-renames` — 2/2 tests pass (`git apply` now distinguishes source vs target paths for rename/copy hunks and snapshots preimage sources across multi-file patches)
- `t4117-apply-reject` — 8/8 tests pass (`git apply --reject` now applies matching hunks, writes `<path>.rej` for rejected hunks, and exits non-zero on partial apply)
- `t4152-am-subjects` — 13/13 tests pass (`git am` now folds wrapped Subject continuations into subject text without injecting blank separator lines; `-k` preserves multiline subject breaks)
- `t4003-diff-rename-1` — 7/7 tests pass (`diff-index -p` now honors `GIT_DIFF_OPTS` context settings such as `--unified=0`, including rename/copy patch output)
- `t4133-apply-filenames` — 4/4 tests pass (`git apply` now validates diff header filename consistency and missing filename metadata before applying hunks)
- `t4039-diff-assume-unchanged` — 4/4 tests pass (`ls-files -v` now parses lowercase tag output and `diff-files` ignores assume-unchanged entries)

## What Remains

No test files are currently marked in progress and 680 remain pending. See `plan.md` for the full prioritized list.
