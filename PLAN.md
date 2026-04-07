# PLAN.md — Grit Test Coverage Work Plan

Generated: 2026-04-05

**Goal:** Make every upstream Git test file fully pass.

**Strategy:** Work through files in priority order. Quick wins first (files with
fewest remaining failures), plumbing before porcelain, external helpers last.

**Workflow:** Pick a file → fix the Rust code → `./scripts/run-tests.sh <file>` →
commit → check it off → move on.

---

## 1. Basic/Setup (37 files)

- [x] `t0050-filesystem` ████████████████████ 13/13 (0 left) — Various filesystem issues
- [x] `t0062-revision-walking` ████████████████████ 2/2 (0 left) — Test revision walking api
- [x] `t0071-sort` ████████████████████ 1/1 (0 left) — verify sort functions
- [x] `t0080-unit-test-output` ████████████████████ 1/1 (0 left) — Test the output of the unit test framework
- [x] `t0056-git-C` ████████████████████ 11/11 (0 left) — 
- [x] `t0007-git-var` ████████████████████ 27/27 (0 left) — basic sanity checks for git var
- [x] `t0009-git-dir-validation` ████████████████████ 6/6 (0 left) — setup: validation of .git file/directory types

- [x] `t0081-find-pack` ████████████████████ 4/4 (0 left) — test `test-tool find-pack`
- [x] `t0030-stripspace` ████████████████████ 30/30 (0 left) — git stripspace
- [x] `t0041-usage` ████████████████████ 16/16 (0 left) — Test commands behavior when given invalid argument value
- [x] `t0004-unwritable` ████████████████████ 9/9 (0 left) — detect unwritable repository and fail correctly
- [x] `t0031-lockfile-pid` ████████████████████ 7/7 (0 left) — lock file PID info tests

- [x] `t0005-signals` ████████████████████ 5/5 (0 left) — signals work as we expect
- [x] `t0068-for-each-repo` ████████████████████ 5/5 (0 left) — git for-each-repo builtin
- [x] `t0018-advice` ████████████████████ 6/6 (0 left) — Test advise_if_enabled functionality
- [x] `t0017-env-helper` ████████████████████ 5/5 (0 left) — test test-tool env-helper
- [x] `t0002-gitfile` ████████████████████ 14/14 (0 left) — .git file

- [x] `t0091-bugreport` ████████████████████ 13/13 (0 left) — git bugreport
- [x] `t0066-dir-iterator` ████████████████████ 10/10 (0 left) — Test the dir-iterator functionality
- [x] `t0067-parse_pathspec_file` ████████████████████ 8/8 (0 left) — Test parse_pathspec_file()
- [x] `t0070-fundamental` ████████████████████ 11/11 (0 left) — check that the most basic functions work

- [x] `t0095-bloom` ████████████████████ 11/11 (0 left) — Testing the various Bloom filter computations in bloom.c
- [x] `t0035-safe-bare-repository` ████████████████████ 12/12 (0 left) — verify safe.bareRepository checks
- [x] `t0033-safe-directory` ████████████████████ 22/22 (0 left) — verify safe.directory checks
- [x] `t0019-json-writer` ████████████████████ 16/16 (0 left) — test json-writer JSON generation
- [x] `t0020-crlf` ████████████████████ 36/36 (0 left) — CRLF conversion
- [x] `t0014-alias` ████████████████████ 21/21 (0 left) — git command aliasing
- [x] `t0061-run-command` ████████████████████ 24/24 (0 left) — Test run command
- [x] `t0090-cache-tree` ████████████████████ 22/22 (0 left) — Test whether cache-tree is properly updated

- [x] `t0021-conversion` ████████████████████ 42/42 (0 left) — blob conversion via gitattributes
- [ ] `t0003-attributes` █████░░░░░░░░░░░░░░░ 15/55 (40 left) — 
- [ ] `t0001-init` ██████████░░░░░░░░░░ 54/102 (48 left) — git init
- [ ] `t0000-basic` █████░░░░░░░░░░░░░░░ 23/92 (69 left) — Test the very basics part #1.

- [ ] `t0040-parse-options` ░░░░░░░░░░░░░░░░░░░░ 0/94 (94 left) — our own option parser
- [ ] `t0012-help` ██░░░░░░░░░░░░░░░░░░ 15/124 (109 left) — help
- [ ] `t0060-path-utils` █░░░░░░░░░░░░░░░░░░░ 18/219 (201 left) — Test various path utilities
- [ ] `t0027-auto-crlf` █████████░░░░░░░░░░░ 1238/2600 (1362 left) — CRLF conversion all combinations

## 2. Plumbing (94 files)

- [x] `t1505-rev-parse-last` ████████████████████ 7/7 (0 left) — test @{-N} syntax
- [x] `t1418-reflog-exists` ████████████████████ 6/6 (0 left) — Test reflog display routines
- [x] `t0213-trace2-ancestry` ████████████████████ 5/5 (0 left) — test trace2 cmd_ancestry event
- [x] `t1100-commit-tree-options` ████████████████████ 5/5 (0 left) — git commit-tree options test

- [x] `t1003-read-tree-prefix` ████████████████████ 3/3 (0 left) — git read-tree --prefix test.

- [x] `t1008-read-tree-overlay` ████████████████████ 2/2 (0 left) — test multi-tree read-tree without merging
- [x] `t0611-reftable-httpd` ████████████████████ 1/1 (0 left) — reftable HTTPD tests
- [x] `t1022-read-tree-partial-clone` ████████████████████ 1/1 (0 left) — git read-tree in partial clones
- [x] `t1402-check-ref-format` ████████████████████ 99/99 (0 left) — Test git check-ref-format
- [x] `t1303-wacky-config` ████████████████████ 11/11 (0 left) — Test wacky input to git config
- [x] `t0101-at-syntax` ████████████████████ 8/8 (0 left) — various @{whatever} syntax tests
- [x] `t1303-wacky-config` ████████████████████ 11/11 (0 left) — Test wacky input to git config
- [x] `t0101-at-syntax` ████████████████████ 8/8 (0 left) — various @{whatever} syntax tests
- [x] `t1015-read-index-unmerged` ████████████████████ 6/6 (0 left) — Test various callers of read_index_unmerged
- [x] `t1310-config-default` ████████████████████ 5/5 (0 left) — Test git config in different settings (with --default)
- [x] `t1601-index-bogus` ████████████████████ 4/4 (0 left) — test handling of bogus index entries
- [x] `t1901-repo-structure` ████████████████████ 4/4 (0 left) — test git repo structure
- [x] `t1311-config-optional` ████████████████████ 3/3 (0 left) — :(optional) paths
- [x] `t1408-packed-refs` ████████████████████ 3/3 (0 left) — packed-refs entries are covered by loose refs
- [x] `t1401-symbolic-ref` ████████████████████ 25/25 (0 left) — basic symbolic-ref tests
- [x] `t1307-config-blob` ████████████████████ 13/13 (0 left) — support for reading config from a blob
- [x] `t1503-rev-parse-verify` ████████████████████ 12/12 (0 left) — test git rev-parse --verify
- [x] `t1600-index` ████████████████████ 7/7 (0 left) — index file specific tests
- [x] `t1407-worktree-ref-store` ████████████████████ 4/4 (0 left) — test worktree ref store api
- [x] `t1412-reflog-loop` ████████████████████ 3/3 (0 left) — reflog walk shows repeated commits again
- [x] `t1512-rev-parse-disambiguation` ████████████████████ 3/3 (0 left) — object name disambiguation

- [x] `t1051-large-conversion` ████████████████████ 12/12 (0 left) — test conversion filters on large files
- [x] `t1012-read-tree-df` ████████████████████ 5/5 (0 left) — read-tree D/F conflict corner cases
- [x] `t0411-clone-from-partial` ████████████████████ 7/7 (0 left) — check that local clone does not fetch from promisor remotes
- [x] `t1090-sparse-checkout-scope` ████████████████████ 7/7 (0 left) — sparse checkout scope tests
- [ ] `t1302-repo-version` █████████████░░░░░░░ 12/18 (6 left) — Test repository version check
- [ ] `t1060-object-corruption` ████████████░░░░░░░░ 11/17 (6 left) — see how we handle various forms of corruption
- [ ] `t1020-subdirectory` ████████████░░░░░░░░ 9/15 (6 left) — Try various core-level commands in subdirectory.

- [ ] `t1415-worktree-refs` ████████░░░░░░░░░░░░ 4/10 (6 left) — per-worktree refs
- [ ] `t1514-rev-parse-push` ██████░░░░░░░░░░░░░░ 3/9 (6 left) — test <branch>@{push} syntax
- [ ] `t1005-read-tree-reset` ██░░░░░░░░░░░░░░░░░░ 1/7 (6 left) — read-tree -u --reset
- [ ] `t0100-previous` ░░░░░░░░░░░░░░░░░░░░ 0/6 (6 left) — previous branch syntax @{-n}
- [ ] `t1511-rev-parse-caret` ███████████░░░░░░░░░ 10/17 (7 left) — tests for ref^{stuff}
- [ ] `t1406-submodule-ref-store` ██████████░░░░░░░░░░ 8/15 (7 left) — test submodule ref store api
- [ ] `t1309-early-config` ██████░░░░░░░░░░░░░░ 3/10 (7 left) — Test read_early_config()
- [ ] `t0614-reftable-fsck` ░░░░░░░░░░░░░░░░░░░░ 0/7 (7 left) — Test reftable backend consistency check
- [ ] `t1416-ref-transaction-hooks` ████░░░░░░░░░░░░░░░░ 2/10 (8 left) — reference transaction hooks
- [ ] `t1014-read-tree-confusing` █████████████░░░░░░░ 19/28 (9 left) — check that read-tree rejects confusing paths
- [ ] `t1417-reflog-updateref` ███████████░░░░░░░░░ 12/21 (9 left) — git reflog --updateref
- [ ] `t1414-reflog-walk` █████░░░░░░░░░░░░░░░ 3/12 (9 left) — various tests of reflog walk (log -g) behavior
- [ ] `t1421-reflog-write` ██░░░░░░░░░░░░░░░░░░ 1/10 (9 left) — Manually write reflog entries
- [ ] `t1403-show-ref` ███░░░░░░░░░░░░░░░░░ 2/12 (10 left) — show-ref
- [ ] `t1306-xdg-files` █████████░░░░░░░░░░░ 10/21 (11 left) — Compatibility with $XDG_CONFIG_HOME/git/ files
- [ ] `t1004-read-tree-m-u-wf` ███████░░░░░░░░░░░░░ 6/17 (11 left) — read-tree -m -u checks working tree files
- [ ] `t1411-reflog-show` ███████░░░░░░░░░░░░░ 6/17 (11 left) — Test reflog display routines
- [ ] `t0212-trace2-event` ░░░░░░░░░░░░░░░░░░░░ 0/11 (11 left) — test trace2 facility
- [ ] `t0613-reftable-write-options` ░░░░░░░░░░░░░░░░░░░░ 0/11 (11 left) — reftable write options
- [ ] `t1419-exclude-refs` █░░░░░░░░░░░░░░░░░░░ 1/13 (12 left) — test exclude_patterns functionality in main ref store
- [ ] `t0211-trace2-perf` ████░░░░░░░░░░░░░░░░ 4/17 (13 left) — test trace2 facility (perf target)
- [ ] `t1508-at-combinations` ████████████░░░░░░░░ 21/35 (14 left) — test various @{X} syntax combinations together
- [ ] `t1405-main-ref-store` ██░░░░░░░░░░░░░░░░░░ 2/16 (14 left) — test main ref store api
- [ ] `t0210-trace2-normal` ░░░░░░░░░░░░░░░░░░░░ 0/14 (14 left) — test trace2 facility (normal target)
- [ ] `t1050-large` █████████░░░░░░░░░░░ 14/29 (15 left) — adding and checking out large blobs
- [ ] `t1507-rev-parse-upstream` ████████░░░░░░░░░░░░ 13/29 (16 left) — test <branch>@{upstream} syntax
- [ ] `t0500-progress-display` ░░░░░░░░░░░░░░░░░░░░ 0/16 (16 left) — progress display
- [ ] `t1506-rev-parse-diagnosis` ████████░░░░░░░░░░░░ 13/30 (17 left) — test git rev-parse diagnosis for invalid argument
- [ ] `t1501-work-tree` ██████████░░░░░░░░░░ 20/39 (19 left) — test separate work tree
- [ ] `t0602-reffiles-fsck` ██░░░░░░░░░░░░░░░░░░ 3/23 (20 left) — Test reffiles backend consistency check
- [ ] `t1002-read-tree-m-u-2way` █░░░░░░░░░░░░░░░░░░░ 2/22 (20 left) — Two way merge with read-tree -m -u $H $M

- [ ] `t1301-shared-repo` █░░░░░░░░░░░░░░░░░░░ 2/22 (20 left) — Test shared repository initialization
- [ ] `t1011-read-tree-sparse-checkout` ░░░░░░░░░░░░░░░░░░░░ 1/23 (22 left) — sparse checkout tests

- [ ] `t0600-reffiles-backend` █████░░░░░░░░░░░░░░░ 9/33 (24 left) — Test reffiles backend
- [ ] `t1007-hash-object` ███████░░░░░░░░░░░░░ 15/40 (25 left) — git hash-object
- [ ] `t1305-config-include` ██████░░░░░░░░░░░░░░ 12/37 (25 left) — test config file include directives
- [ ] `t1001-read-tree-m-2way` ██░░░░░░░░░░░░░░░░░░ 4/29 (25 left) — Two way merge with read-tree -m $H $M

- [ ] `t1502-rev-parse-parseopt` █████░░░░░░░░░░░░░░░ 11/37 (26 left) — test git rev-parse --parseopt
- [ ] `t1700-split-index` █░░░░░░░░░░░░░░░░░░░ 2/28 (26 left) — split index mode tests
- [x] `t1900-repo-info` ████████████████████ 37/37 (0 left) — test git repo-info
- [ ] `t0410-partial-clone` ████░░░░░░░░░░░░░░░░ 9/38 (29 left) — partial clone
- [ ] `t1701-racy-split-index` ░░░░░░░░░░░░░░░░░░░░ 1/31 (30 left) — racy split index
- [ ] `t1423-ref-backend` ██░░░░░░░░░░░░░░░░░░ 4/36 (32 left) — Test reference backend URIs
- [ ] `t1430-bad-ref-name` ███░░░░░░░░░░░░░░░░░ 8/42 (34 left) — Test handling of ref names that check-ref-format rejects
- [ ] `t1504-ceiling-dirs` ███░░░░░░░░░░░░░░░░░ 8/44 (36 left) — test GIT_CEILING_DIRECTORIES
- [ ] `t1460-refs-migrate` ░░░░░░░░░░░░░░░░░░░░ 1/37 (36 left) — migration of ref storage backends
- [ ] `t1410-reflog` █░░░░░░░░░░░░░░░░░░░ 4/41 (37 left) — Test prune and reflog expiration
- [ ] `t1308-config-set` ░░░░░░░░░░░░░░░░░░░░ 1/39 (38 left) — Test git config-set API in different settings
- [ ] `t1404-update-ref-errors` ░░░░░░░░░░░░░░░░░░░░ 0/38 (38 left) — Test git update-ref error handling
- [ ] `t1000-read-tree-m-3way` ██████████░░░░░░░░░░ 44/83 (39 left) — Three way merge with read-tree -m

- [ ] `t1800-hook` ██░░░░░░░░░░░░░░░░░░ 5/44 (39 left) — git-hook command and config-managed multihooks
- [ ] `t1500-rev-parse` █████████░░░░░░░░░░░ 38/81 (43 left) — test git rev-parse
- [ ] `t0301-credential-cache` ██░░░░░░░░░░░░░░░░░░ 6/52 (46 left) — credential-cache tests
- [ ] `t0300-credentials` █░░░░░░░░░░░░░░░░░░░ 3/56 (53 left) — basic credential helper tests
- [ ] `t0302-credential-store` █░░░░░░░░░░░░░░░░░░░ 5/65 (60 left) — credential-store tests
- [ ] `t1451-fsck-buffer` ██░░░░░░░░░░░░░░░░░░ 10/72 (62 left) — fsck on buffers without NUL termination

- [ ] `t1091-sparse-checkout-builtin` ██░░░░░░░░░░░░░░░░░░ 10/77 (67 left) — sparse checkout builtin tests
- [ ] `t0610-reftable-basics` ████░░░░░░░░░░░░░░░░ 21/91 (70 left) — reftable basics
- [ ] `t1006-cat-file` ███████████████░░░░░ 220/291 (71 left) — git cat-file
- [ ] `t1510-repo-setup` ██░░░░░░░░░░░░░░░░░░ 12/109 (97 left) — Tests of cwd/prefix/worktree/gitdir setup in all cases

- [ ] `t1517-outside-repo` █████████░░░░░░░░░░░ 97/195 (98 left) — check random commands outside repo
- [ ] `t1092-sparse-checkout-compatibility` ░░░░░░░░░░░░░░░░░░░░ 3/106 (103 left) — compare full workdir to sparse workdir
- [ ] `t0450-txt-doc-vs-help` ██████████████░░░░░░ 401/554 (153 left) — assert (unbuilt) Documentation/*.adoc and -h output


## 3. Index/Checkout (50 files)

- [x] `t2060-switch` ████████████████████ 16/16 — switch basic functionality
- [x] `t2050-git-dir-relative` ████████████████████ 4/4 (0 left) — check problems with relative GIT_DIR

- [x] `t2015-checkout-unborn` ████████████████████ 6/6 (0 left) — checkout from unborn branch
- [x] `t2105-update-index-gitfile` ████████████████████ 4/4 (0 left) — git update-index for gitlink to .git file.

- [x] `t2012-checkout-last` ████████████████████ 22/22 (0 left) — checkout can switch to last branch and merge base
- [x] `t2010-checkout-ambiguous` ████████████████████ 10/10 (0 left) — checkout and pathspecs/refspecs ambiguities
- [x] `t2104-update-index-skip-worktree` ████████████████████ 7/7 (0 left) — skip-worktree bit test
- [x] `t2023-checkout-m` ████████████████████ 5/5 (0 left) — checkout -m -- <conflicted path>

- [x] `t2027-checkout-track` ████████████████████ 5/5 (0 left) — tests for git branch --track
- [x] `t2202-add-addremove` ████████████████████ 3/3 (0 left) — git add --all
- [x] `t2018-checkout-branch` ████████████████████ 25/25 (0 left) — checkout
- [x] `t2006-checkout-index-basic` ████████████████████ 9/9 (0 left) — basic checkout-index tests

- [x] `t2019-checkout-ambiguous-ref` ████████████████████ 9/9 (0 left) — checkout handling of ambiguous (branch/tag) refs
- [x] `t2206-add-submodule-ignored` ████████████████████ 8/8 (0 left) — git add respects submodule ignore=all and explicit pathspec
- [x] `t2022-checkout-paths` ████████████████████ 5/5 (0 left) — checkout $tree -- $paths
- [x] `t2082-parallel-checkout-attributes` ████████████████████ 5/5 (0 left) — parallel-checkout: attributes

- [x] `t2103-update-index-ignore-missing` ████████████████████ 5/5 (0 left) — update-index with options
- [x] `t2000-conflict-when-checking-files-out` ████████████████████ 14/14 (0 left) — git conflicts when checking files out test.
- [x] `t2011-checkout-invalid-head` ████████████████████ 10/10 (0 left) — checkout switching away from an invalid branch
- [x] `t2107-update-index-basic` ████████████████████ 10/10 (0 left) — basic update-index tests

- [x] `t2021-checkout-overwrite` ████████████████████ 9/9 (0 left) — checkout must not overwrite an untracked objects
- [x] `t2025-checkout-no-overlay` ████████████████████ 6/6 (0 left) — checkout --no-overlay <tree-ish> -- <pathspec>
- [x] `t2201-add-update-typechange` ████████████████████ 6/6 (0 left) — more git add -u
- [x] `t2200-add-update` ████████████████████ 19/19 (0 left) — git add -u

- [x] `t2500-untracked-overwriting` ████████████████████ 10/10 (0 left) — Test handling of overwriting untracked files
- [x] `t2108-update-index-refresh-racy` ████████████████████ 6/6 (0 left) — update-index refresh tests related to racy timestamps
- [x] `t2017-checkout-orphan` ████████████████████ 13/13 (0 left) — git checkout --orphan

- [x] `t2003-checkout-cache-mkdir` ████████████████████ 10/10 (0 left) — git checkout-index --prefix test.

- [ ] `t2401-worktree-prune` ███████░░░░░░░░░░░░░ 5/13 (8 left) — prune $GIT_DIR/worktrees
- [ ] `t2404-worktree-config` ██████░░░░░░░░░░░░░░ 4/12 (8 left) — config file in multi worktree
- [ ] `t2405-worktree-submodule` █████░░░░░░░░░░░░░░░ 3/11 (8 left) — Combination of submodules and multiple worktrees
- [ ] `t2020-checkout-detach` █████████████░░░░░░░ 17/26 (9 left) — checkout into detached HEAD state
- [ ] `t2204-add-ignored` ███████████████░░░░░ 37/47 (10 left) — giving ignored paths to git add
- [ ] `t2070-restore` ██████░░░░░░░░░░░░░░ 5/15 (10 left) — restore basic functionality
- [ ] `t2205-add-worktree-config` ████░░░░░░░░░░░░░░░░ 3/13 (10 left) — directory traversal respects user config

- [ ] `t2072-restore-pathspec-file` ███░░░░░░░░░░░░░░░░░ 2/12 (10 left) — restore --pathspec-from-file
- [ ] `t2026-checkout-pathspec-file` █░░░░░░░░░░░░░░░░░░░ 1/11 (10 left) — checkout --pathspec-from-file
- [ ] `t2407-worktree-heads` █░░░░░░░░░░░░░░░░░░░ 1/12 (11 left) — test operations trying to overwrite refs at worktree HEAD
- [ ] `t2080-parallel-checkout-basics` ░░░░░░░░░░░░░░░░░░░░ 0/11 (11 left) — parallel-checkout basics

- [ ] `t2016-checkout-patch` ███████░░░░░░░░░░░░░ 7/19 (12 left) — git checkout --patch
- [ ] `t2030-unresolve-info` ██░░░░░░░░░░░░░░░░░░ 2/14 (12 left) — undoing resolution
- [ ] `t2071-restore-patch` ██░░░░░░░░░░░░░░░░░░ 2/15 (13 left) — git restore --patch
- [ ] `t2203-add-intent` ████░░░░░░░░░░░░░░░░ 4/19 (15 left) — Intent to add
- [ ] `t2004-checkout-cache-temp` ██████░░░░░░░░░░░░░░ 7/23 (16 left) — git checkout-index --temp test.

- [ ] `t2402-worktree-list` █████░░░░░░░░░░░░░░░ 8/27 (19 left) — test git worktree list
- [ ] `t2406-worktree-repair` ███░░░░░░░░░░░░░░░░░ 4/24 (20 left) — test git worktree repair
- [ ] `t2501-cwd-empty` ██░░░░░░░░░░░░░░░░░░ 3/24 (21 left) — Test handling of the current working directory becoming empty
- [ ] `t2024-checkout-dwim` █░░░░░░░░░░░░░░░░░░░ 2/23 (21 left) — checkout <branch>

- [ ] `t2013-checkout-submodule` ███░░░░░░░░░░░░░░░░░ 14/74 (60 left) — checkout can handle submodules
- [ ] `t2400-worktree-add` ███░░░░░░░░░░░░░░░░░ 40/232 (192 left) — test git worktree add

## 4. Core Commands (109 files)

- [ ] `t3302-notes-index-expensive` ██████████████████░░ 11/12 (1 left) — Test commit notes index (expensive!)
- [ ] `t3502-cherry-pick-merge` ██████████████████░░ 11/12 (1 left) — cherry picking and reverting a merge

- [ ] `t3211-peel-ref` █████████████████░░░ 7/8 (1 left) — tests for the peel_ref optimization of packed-refs
- [ ] `t3003-ls-files-exclude` █████████████████░░░ 6/7 (1 left) — ls-files --exclude does not affect index files
- [ ] `t3004-ls-files-basic` ████████████████░░░░ 5/6 (1 left) — basic ls-files tests

- [ ] `t3304-notes-mixed` ████████████████░░░░ 5/6 (1 left) — Test notes trees that also contain non-notes
- [ ] `t3102-ls-tree-wildcards` ███████████████░░░░░ 3/4 (1 left) — ls-tree with(out) globs
- [ ] `t3500-cherry` ███████████████░░░░░ 3/4 (1 left) — git cherry should detect patches integrated upstream

- [ ] `t3009-ls-files-others-nonsubmodule` ██████████░░░░░░░░░░ 1/2 (1 left) — test git ls-files --others with non-submodule repositories

- [ ] `t3908-stash-in-worktree` ██████████░░░░░░░░░░ 1/2 (1 left) — Test git stash in a worktree
- [ ] `t3008-ls-files-lazy-init-name-hash` ░░░░░░░░░░░░░░░░░░░░ 0/1 (1 left) — Test the lazy init name hash with various folder structures
- [ ] `t3205-branch-color` ██████████░░░░░░░░░░ 2/4 (2 left) — basic branch output coloring
- [ ] `t3012-ls-files-dedup` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — git ls-files --deduplicate test
- [ ] `t3307-notes-man` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — Examples from the git-notes man page

- [ ] `t3423-rebase-reword` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — git rebase interactive with rewording
- [ ] `t3702-add-edit` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — add -e basic tests
- [ ] `t3450-history` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — tests for git-history command
- [ ] `t3305-notes-fanout` ███████████░░░░░░░░░ 4/7 (3 left) — Test that adding/removing many notes triggers automatic fanout restructuring
- [ ] `t3005-ls-files-relative` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — ls-files tests with relative paths

- [ ] `t3427-rebase-subtree` ░░░░░░░░░░░░░░░░░░░░ 0/3 (3 left) — git rebase tests for -Xsubtree

- [ ] `t3506-cherry-pick-ff` ████████████░░░░░░░░ 7/11 (4 left) — test cherry-picking with --ff option
- [ ] `t3103-ls-tree-misc` ████████████░░░░░░░░ 6/10 (4 left) — 

- [ ] `t3401-rebase-and-am-rename` ████████████░░░░░░░░ 6/10 (4 left) — git rebase + directory rename tests
- [ ] `t3419-rebase-patch-id` ██████████░░░░░░░░░░ 4/8 (4 left) — git rebase - test patch id computation
- [ ] `t3429-rebase-edit-todo` ████████░░░░░░░░░░░░ 3/7 (4 left) — rebase should reread the todo file if an exec modifies it
- [ ] `t3703-add-magic-pathspec` ██████░░░░░░░░░░░░░░ 2/6 (4 left) — magic pathspec tests using git-add
- [ ] `t3601-rm-pathspec-file` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — rm --pathspec-from-file
- [ ] `t3909-stash-pathspec-file` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — stash --pathspec-from-file
- [ ] `t3417-rebase-whitespace-fix` ░░░░░░░░░░░░░░░░░░░░ 0/4 (4 left) — git rebase --whitespace=fix

- [ ] `t3433-rebase-across-mode-change` ░░░░░░░░░░░░░░░░░░░░ 0/4 (4 left) — git rebase across mode change
- [ ] `t3040-subprojects-basic` ██████████░░░░░░░░░░ 6/11 (5 left) — Basic subproject functionality
- [ ] `t3320-notes-merge-worktrees` ████████░░░░░░░░░░░░ 4/9 (5 left) — Test merging of notes trees in multiple worktrees
- [ ] `t3010-ls-files-killed-modified` ███░░░░░░░░░░░░░░░░░ 1/6 (5 left) — git ls-files -k and -m flags test.

- [ ] `t3503-cherry-pick-root` ███░░░░░░░░░░░░░░░░░ 1/6 (5 left) — test cherry-picking (and reverting) a root commit
- [ ] `t3101-ls-tree-dirname` █████████████░░░░░░░ 13/19 (6 left) — git ls-tree directory and filenames handling.

- [ ] `t3906-stash-submodule` ████████████░░░░░░░░ 10/16 (6 left) — stash can handle submodules
- [ ] `t3904-stash-patch` ████████░░░░░░░░░░░░ 4/10 (6 left) — stash -p
- [ ] `t3504-cherry-pick-rerere` ██████░░░░░░░░░░░░░░ 3/9 (6 left) — cherry-pick should rerere for conflicts
- [ ] `t3509-cherry-pick-merge-df` ██████░░░░░░░░░░░░░░ 3/9 (6 left) — Test cherry-pick with directory/file conflicts
- [ ] `t3060-ls-files-with-tree` █████░░░░░░░░░░░░░░░ 2/8 (6 left) — git ls-files test (--with-tree).

- [ ] `t3428-rebase-signoff` ██░░░░░░░░░░░░░░░░░░ 1/7 (6 left) — git rebase --signoff

- [ ] `t3505-cherry-pick-empty` ██████████░░░░░░░░░░ 9/17 (8 left) — test cherry-picking an empty commit
- [ ] `t3902-quoted` ███████░░░░░░░░░░░░░ 5/13 (8 left) — quoted output
- [ ] `t3306-notes-prune` ██████░░░░░░░░░░░░░░ 4/12 (8 left) — Test git notes prune
- [ ] `t3438-rebase-broken-files` ██░░░░░░░░░░░░░░░░░░ 1/9 (8 left) — rebase behavior when on-disk files are broken
- [ ] `t3201-branch-contains` ████████████░░░░░░░░ 15/24 (9 left) — branch --contains <commit>, --no-contains <commit> --merged, and --no-merged
- [ ] `t3104-ls-tree-format` ██████████░░░░░░░░░░ 10/19 (9 left) — ls-tree --format
- [ ] `t3204-branch-name-interpretation` ████████░░░░░░░░░░░░ 7/16 (9 left) — interpreting exotic branch name arguments

- [ ] `t3000-ls-files-others` ████████░░░░░░░░░░░░ 6/15 (9 left) — basic tests for ls-files --others

- [ ] `t3508-cherry-pick-many-commits` ███████░░░░░░░░░░░░░ 5/14 (9 left) — test cherry-picking many commits
- [ ] `t3704-add-pathspec-file` ███░░░░░░░░░░░░░░░░░ 2/11 (9 left) — add --pathspec-from-file
- [ ] `t3440-rebase-trailer` ██░░░░░░░░░░░░░░░░░░ 1/10 (9 left) — git rebase --trailer integration tests

- [ ] `t3907-stash-show-config` ██░░░░░░░░░░░░░░░░░░ 1/10 (9 left) — Test git stash show configuration.
- [ ] `t3413-rebase-hook` ██████░░░░░░░░░░░░░░ 5/15 (10 left) — git rebase with its hook(s)
- [ ] `t3303-notes-subtrees` ██████████░░░░░░░░░░ 12/23 (11 left) — Test commit notes organized in subtrees
- [ ] `t3416-rebase-onto-threedots` ███████░░░░░░░░░░░░░ 7/18 (11 left) — git rebase --onto A...B
- [ ] `t3402-rebase-merge` ███░░░░░░░░░░░░░░░░░ 2/13 (11 left) — git rebase --merge test
- [ ] `t3602-rm-sparse-checkout` ███░░░░░░░░░░░░░░░░░ 2/13 (11 left) — git rm in sparse checked out working trees
- [ ] `t3013-ls-files-format` ████████░░░░░░░░░░░░ 8/20 (12 left) — git ls-files --format test
- [ ] `t3425-rebase-topology-merges` █░░░░░░░░░░░░░░░░░░░ 1/13 (12 left) — rebase topology tests with merges
- [ ] `t3412-rebase-root` █████████░░░░░░░░░░░ 12/25 (13 left) — git rebase --root

- [ ] `t3300-funny-names` ███████░░░░░░░░░░░░░ 8/21 (13 left) — Pathnames with funny characters.

- [ ] `t3512-cherry-pick-submodule` ██░░░░░░░░░░░░░░░░░░ 2/15 (13 left) — cherry-pick can handle submodules
- [ ] `t3437-rebase-fixup-options` ░░░░░░░░░░░░░░░░░░░░ 0/13 (13 left) — git rebase interactive fixup options

- [ ] `t3422-rebase-incompatible-options` ██████████████░░░░░░ 38/52 (14 left) — test if rebase detects and aborts on incompatible options
- [ ] `t3011-common-prefixes-and-directory-traversal` ██████░░░░░░░░░░░░░░ 7/21 (14 left) — directory traversal handling, especially with common prefixes
- [ ] `t3451-history-reword` ░░░░░░░░░░░░░░░░░░░░ 0/14 (14 left) — tests for git-history reword subcommand
- [ ] `t3920-crlf-messages` ███░░░░░░░░░░░░░░░░░ 3/18 (15 left) — Test ref-filter and pretty APIs for commit and tag messages using CRLF
- [ ] `t3001-ls-files-others-exclude` ████████░░░░░░░░░░░░ 11/27 (16 left) — git ls-files --others --exclude

- [ ] `t3403-rebase-skip` ████░░░░░░░░░░░░░░░░ 4/20 (16 left) — git rebase --merge --skip tests
- [ ] `t3308-notes-merge` ███░░░░░░░░░░░░░░░░░ 3/19 (16 left) — Test merging of notes trees
- [ ] `t3700-add` ██████████████░░░░░░ 41/58 (17 left) — Test of git add, including the -- option.
- [ ] `t3501-revert-cherry-pick` ███░░░░░░░░░░░░░░░░░ 4/21 (17 left) — miscellaneous basic tests for cherry-pick and revert
- [ ] `t3424-rebase-empty` ███░░░░░░░░░░░░░░░░░ 3/20 (17 left) — git rebase of commits that start or become empty
- [ ] `t3407-rebase-abort` ░░░░░░░░░░░░░░░░░░░░ 0/17 (17 left) — git rebase --abort tests
- [ ] `t3310-notes-merge-manual-resolve` ███░░░░░░░░░░░░░░░░░ 4/22 (18 left) — Test notes merging with manual conflict resolution
- [ ] `t3207-branch-submodule` ██░░░░░░░░░░░░░░░░░░ 2/20 (18 left) — git branch submodule tests
- [ ] `t3705-add-sparse-checkout` ██░░░░░░░░░░░░░░░░░░ 2/20 (18 left) — git add in sparse checked out working trees
- [ ] `t3436-rebase-more-options` █░░░░░░░░░░░░░░░░░░░ 1/19 (18 left) — tests to ensure compatibility between am and interactive backends
- [ ] `t3905-stash-include-untracked` ███████░░░░░░░░░░░░░ 12/34 (22 left) — Test git stash --include-untracked
- [ ] `t3511-cherry-pick-x` ░░░░░░░░░░░░░░░░░░░░ 0/22 (22 left) — Test cherry-pick -x and -s
- [ ] `t3202-show-branch` ██░░░░░░░░░░░░░░░░░░ 4/27 (23 left) — test show-branch
- [ ] `t3431-rebase-fork-point` ██░░░░░░░░░░░░░░░░░░ 3/26 (23 left) — git rebase --fork-point test
- [ ] `t3007-ls-files-recurse-submodules` ░░░░░░░░░░░░░░░░░░░░ 1/24 (23 left) — Test ls-files recurse-submodules feature

- [ ] `t3311-notes-merge-fanout` ░░░░░░░░░░░░░░░░░░░░ 1/24 (23 left) — Test notes merging at various fanout levels
- [ ] `t3418-rebase-continue` ████░░░░░░░░░░░░░░░░ 6/30 (24 left) — git rebase --continue tests
- [ ] `t3650-replay-basics` ███░░░░░░░░░░░░░░░░░ 6/31 (25 left) — basic git replay tests
- [ ] `t3426-rebase-submodule` ██░░░░░░░░░░░░░░░░░░ 4/29 (25 left) — rebase can handle submodules
- [ ] `t3452-history-split` ░░░░░░░░░░░░░░░░░░░░ 0/25 (25 left) — tests for git-history split subcommand
- [ ] `t3203-branch-output` ███████░░░░░░░░░░░░░ 15/41 (26 left) — git branch display tests
- [ ] `t3415-rebase-autosquash` █░░░░░░░░░░░░░░░░░░░ 2/28 (26 left) — auto squash
- [ ] `t3321-notes-stripspace` ░░░░░░░░░░░░░░░░░░░░ 1/27 (26 left) — Test commit notes with stripspace behavior
- [ ] `t3406-rebase-message` █░░░░░░░░░░░░░░░░░░░ 3/32 (29 left) — messages from rebase operation
- [ ] `t3309-notes-merge-auto-resolve` █░░░░░░░░░░░░░░░░░░░ 2/31 (29 left) — Test notes merging with auto-resolving strategies
- [ ] `t3400-rebase` ████░░░░░░░░░░░░░░░░ 9/39 (30 left) — git rebase assorted tests

- [ ] `t3600-rm` ████████████░░░░░░░░ 50/82 (32 left) — Test of the various options to git rm.
- [ ] `t3510-cherry-pick-sequence` ████████░░░░░░░░░░░░ 22/55 (33 left) — Test cherry-pick continuation features

- [ ] `t3507-cherry-pick-conflict` █████░░░░░░░░░░░░░░░ 11/44 (33 left) — test cherry-pick and revert with conflicts

- [ ] `t3430-rebase-merges` ░░░░░░░░░░░░░░░░░░░░ 1/34 (33 left) — git rebase -i --rebase-merges

- [ ] `t3432-rebase-fast-forward` ████████████████░░░░ 189/225 (36 left) — ensure rebase fast-forwards commits when possible
- [ ] `t3206-range-diff` ██░░░░░░░░░░░░░░░░░░ 5/48 (43 left) — range-diff tests
- [ ] `t3105-ls-tree-output` ████░░░░░░░░░░░░░░░░ 13/60 (47 left) — ls-tree output
- [ ] `t3420-rebase-autostash` █░░░░░░░░░░░░░░░░░░░ 4/52 (48 left) — git rebase --autostash tests
- [ ] `t3421-rebase-topology-linear` ███░░░░░░░░░░░░░░░░░ 11/64 (53 left) — basic rebase topology tests
- [ ] `t3800-mktag` █████████░░░░░░░░░░░ 68/151 (83 left) — git mktag: tag object verify test
- [ ] `t3903-stash` ████████░░░░░░░░░░░░ 57/142 (85 left) — Test git stash
- [ ] `t3200-branch` █████████░░░░░░░░░░░ 79/167 (88 left) — git branch assorted tests
- [ ] `t3701-add-interactive` ████░░░░░░░░░░░░░░░░ 28/130 (102 left) — add -i basic tests
- [ ] `t3301-notes` ██████░░░░░░░░░░░░░░ 46/153 (107 left) — Test commit notes

## 5. Diff (132 files)

- [ ] `t4204-patch-id` ███████████████████░ 25/26 (1 left) — git patch-id
- [ ] `t4021-format-patch-numbered` ██████████████████░░ 13/14 (1 left) — Format-patch numbering options
- [ ] `t4065-diff-anchored` █████████████████░░░ 6/7 (1 left) — anchored diff algorithm
- [ ] `t4036-format-patch-signer-mime` ████████████████░░░░ 4/5 (1 left) — format-patch -s should force MIME encoding as needed
- [ ] `t4004-diff-rename-symlink` ███████████████░░░░░ 3/4 (1 left) — More rename detection tests.

- [ ] `t4005-diff-rename-2` ███████████████░░░░░ 3/4 (1 left) — Same rename detection as t4003 but testing diff-raw.
- [ ] `t4043-diff-rename-binary` █████████████░░░░░░░ 2/3 (1 left) — Move a binary file
- [ ] `t4113-apply-ending` █████████████░░░░░░░ 2/3 (1 left) — git apply trying to add an ending line.

- [ ] `t4025-hunk-header` ██████████░░░░░░░░░░ 1/2 (1 left) — diff hunk header truncation
- [ ] `t4066-diff-emit-delay` ██████████░░░░░░░░░░ 1/2 (1 left) — test combined/stat/moved interaction
- [ ] `t4123-apply-shrink` ██████████░░░░░░░░░░ 1/2 (1 left) — apply a patch that is larger than the preimage
- [ ] `t4134-apply-submodule` ██████████░░░░░░░░░░ 1/2 (1 left) — git apply submodule tests
- [ ] `t4256-am-format-flowed` ██████████░░░░░░░░░░ 1/2 (1 left) — test format=flowed support of git am
- [ ] `t4029-diff-trailing-space` ░░░░░░░░░░░░░░░░░░░░ 0/1 (1 left) — diff honors config option, diff.suppressBlankEmpty
- [ ] `t4110-apply-scan` ░░░░░░░░░░░░░░░░░░░░ 0/1 (1 left) — git apply test for patches which require scanning forwards and backwards.

- [ ] `t4007-rename-3` ████████████████░░░░ 11/13 (2 left) — Rename interaction with pathspec.

- [ ] `t4111-apply-subdir` ████████████████░░░░ 8/10 (2 left) — patching from inconvenient places
- [ ] `t4006-diff-mode` ██████████████░░░░░░ 5/7 (2 left) — Test mode change diffs.

- [ ] `t4073-diff-stat-name-width` █████████████░░░░░░░ 4/6 (2 left) — git-diff check diffstat filepaths length when containing UTF-8 chars
- [ ] `t4125-apply-ws-fuzz` ██████████░░░░░░░░░░ 2/4 (2 left) — applying patch that has broken whitespaces in context
- [ ] `t4028-format-patch-mime-headers` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — format-patch mime headers and extra headers do not conflict
- [ ] `t4062-diff-pickaxe` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — Pickaxe options
- [ ] `t4131-apply-fake-ancestor` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — git apply --build-fake-ancestor handling.
- [ ] `t4217-log-limit` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — git log with filter options limiting the output
- [ ] `t4044-diff-index-unique-abbrev` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — test unique sha1 abbreviation on 
- [ ] `t4112-apply-renames` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — git apply should not get confused with rename/copy.

- [ ] `t4152-am-subjects` ███████████████░░░░░ 10/13 (3 left) — test subject preservation with format-patch | am
- [ ] `t4117-apply-reject` ████████████░░░░░░░░ 5/8 (3 left) — git apply with rejects

- [ ] `t4003-diff-rename-1` ███████████░░░░░░░░░ 4/7 (3 left) — More rename detection

- [ ] `t4016-diff-quote` ████████░░░░░░░░░░░░ 2/5 (3 left) — Quoting paths in diff output.

- [ ] `t4018-diff-funcname` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — Test custom diff function name patterns
- [ ] `t4039-diff-assume-unchanged` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — diff with assume-unchanged entries
- [ ] `t4049-diff-stat-count` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — diff --stat-count
- [ ] `t4133-apply-filenames` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — git apply filename consistency check
- [ ] `t4257-am-interactive` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — am --interactive tests
- [ ] `t4258-am-quoted-cr` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — test am --quoted-cr=<action>
- [ ] `t4072-diff-max-depth` ██████████████████░░ 72/76 (4 left) — check that diff --max-depth will limit recursion
- [ ] `t4040-whitespace-status` ████████████░░░░░░░░ 7/11 (4 left) — diff --exit-code with whitespace
- [ ] `t4107-apply-ignore-whitespace` ████████████░░░░░░░░ 7/11 (4 left) — git-apply --ignore-whitespace.
- [ ] `t4127-apply-same-fn` ████████░░░░░░░░░░░░ 3/7 (4 left) — apply same filename
- [ ] `t4206-log-follow-harder-copies` ████████░░░░░░░░░░░░ 3/7 (4 left) — Test --follow should always find copies hard in git log.

- [ ] `t4136-apply-check` ██████░░░░░░░░░░░░░░ 2/6 (4 left) — git apply should exit non-zero with unrecognized input.
- [ ] `t4102-apply-rename` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — git apply handling copy/rename patch.

- [ ] `t4138-apply-ws-expansion` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — git apply test patches with whitespace expansion.
- [ ] `t4023-diff-rename-typechange` ░░░░░░░░░░░░░░░░░░░░ 0/4 (4 left) — typechange rename detection
- [ ] `t4057-diff-combined-paths` ░░░░░░░░░░░░░░░░░░░░ 0/4 (4 left) — combined diff show only paths that are different to all parents
- [ ] `t4074-diff-shifted-matched-group` ░░░░░░░░░░░░░░░░░░░░ 0/4 (4 left) — shifted diff groups re-diffing during histogram diff
- [ ] `t4207-log-decoration-colors` ░░░░░░░░░░░░░░░░░░░░ 0/4 (4 left) — test 
- [ ] `t4055-diff-context` ██████████░░░░░░░░░░ 5/10 (5 left) — diff.context configuration
- [ ] `t4064-diff-oidfind` ██████████░░░░░░░░░░ 5/10 (5 left) — test finding specific blobs in the revision walking
- [ ] `t4031-diff-rewrite-binary` ███████░░░░░░░░░░░░░ 3/8 (5 left) — rewrite diff on binary file
- [ ] `t4126-apply-empty` ███████░░░░░░░░░░░░░ 3/8 (5 left) — apply empty
- [ ] `t4116-apply-reverse` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — git apply in reverse

- [ ] `t4140-apply-ita` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — git apply of i-t-a file
- [ ] `t4153-am-resume-override-opts` ███░░░░░░░░░░░░░░░░░ 1/6 (5 left) — git-am command-line options override saved options
- [ ] `t4104-apply-boundary` ███████████████░░░░░ 18/24 (6 left) — git apply boundary tests
- [ ] `t4001-diff-rename` ██████████████░░░░░░ 17/23 (6 left) — Test rename detection in diff engine.
- [ ] `t4010-diff-pathspec` ████████████░░░░░░░░ 11/17 (6 left) — Pathspec restrictions

- [ ] `t4122-apply-symlink-inside` ██░░░░░░░░░░░░░░░░░░ 1/7 (6 left) — apply to deeper directory without getting fooled with symlink
- [ ] `t4253-am-keep-cr-dos` ██░░░░░░░░░░░░░░░░░░ 1/7 (6 left) — git-am mbox with dos line ending.

- [ ] `t4054-diff-bogus-tree` ██████████░░░░░░░░░░ 7/14 (7 left) — test diff with a bogus tree containing the null sha1
- [ ] `t4114-apply-typechange` ████████░░░░░░░░░░░░ 5/12 (7 left) — git apply should not get confused with type changes.

- [ ] `t4022-diff-rewrite` ███████░░░░░░░░░░░░░ 4/11 (7 left) — rewrite diff
- [ ] `t4033-diff-patience` ███████░░░░░░░░░░░░░ 4/11 (7 left) — patience diff algorithm
- [ ] `t4105-apply-fuzz` ████░░░░░░░░░░░░░░░░ 2/9 (7 left) — apply with fuzz and offset
- [ ] `t4011-diff-symlink` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — Test diff of symlinks.

- [ ] `t4042-diff-textconv-caching` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — test textconv caching
- [ ] `t4046-diff-unmerged` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — diff with unmerged index entries
- [ ] `t4059-diff-submodule-not-initialized` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — Test for submodule diff on non-checked out submodule

- [ ] `t4115-apply-symlink` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — git apply symlinks and partial files

- [ ] `t4252-am-options` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — git am with options and not losing them
- [ ] `t4070-diff-pairs` ░░░░░░░░░░░░░░░░░░░░ 0/7 (7 left) — basic diff-pairs tests
- [ ] `t4017-diff-retval` ███████████████░░░░░ 30/38 (8 left) — Return value of diffs
- [ ] `t4035-diff-quiet` █████████████░░░░░░░ 15/23 (8 left) — Return value of diffs
- [ ] `t4213-log-tabexpand` ██░░░░░░░░░░░░░░░░░░ 1/9 (8 left) — log/show --expand-tabs
- [ ] `t4058-diff-duplicates` ████████░░░░░░░░░░░░ 7/16 (9 left) — test tree diff when trees have duplicate entries
- [ ] `t4008-diff-break-rewrite` ███████░░░░░░░░░░░░░ 5/14 (9 left) — Break and then rename

- [ ] `t4120-apply-popt` █████░░░░░░░░░░░░░░░ 3/12 (9 left) — git apply -p handling.
- [ ] `t4067-diff-partial-clone` ░░░░░░░░░░░░░░░░░░░░ 0/9 (9 left) — behavior of diff when reading objects in a partial clone
- [ ] `t4208-log-magic-pathspec` ██████████░░░░░░░░░░ 11/21 (10 left) — magic pathspec tests using git-log
- [ ] `t4212-log-corrupt` ████░░░░░░░░░░░░░░░░ 3/13 (10 left) — git log with invalid commit headers
- [ ] `t4128-apply-root` ███░░░░░░░░░░░░░░░░░ 2/12 (10 left) — apply same filename
- [ ] `t4139-apply-escape` ███░░░░░░░░░░░░░░░░░ 2/12 (10 left) — paths written by git-apply cannot escape the working tree
- [ ] `t4119-apply-config` █░░░░░░░░░░░░░░░░░░░ 1/11 (10 left) — git apply --whitespace=strip and configuration file.

- [ ] `t4215-log-skewed-merges` ░░░░░░░░░░░░░░░░░░░░ 0/10 (10 left) — git log --graph of skewed merges
- [ ] `t4129-apply-samemode` ██████████░░░░░░░░░░ 12/23 (11 left) — applying patch with mode bits
- [ ] `t4048-diff-combined-binary` ████░░░░░░░░░░░░░░░░ 3/14 (11 left) — combined and merge diff handle binary files and textconv
- [ ] `t4012-diff-binary` ███░░░░░░░░░░░░░░░░░ 2/13 (11 left) — Binary diff and apply

- [ ] `t4056-diff-order` ████████░░░░░░░░░░░░ 10/23 (13 left) — diff order & rotate
- [ ] `t4132-apply-removal` █░░░░░░░░░░░░░░░░░░░ 1/14 (13 left) — git-apply notices removal patches generated by GNU diff
- [ ] `t4108-apply-threeway` ████░░░░░░░░░░░░░░░░ 4/18 (14 left) — git apply --3way
- [ ] `t4100-apply-stat` ████████░░░░░░░░░░░░ 10/25 (15 left) — git apply --stat --summary test, with --recount

- [ ] `t4069-remerge-diff` █░░░░░░░░░░░░░░░░░░░ 1/16 (15 left) — remerge-diff handling
- [ ] `t4019-diff-wserror` ████░░░░░░░░░░░░░░░░ 5/21 (16 left) — diff whitespace error detection
- [ ] `t4063-diff-blobs` ██░░░░░░░░░░░░░░░░░░ 2/18 (16 left) — test direct comparison of blobs via git-diff
- [ ] `t4214-log-graph-octopus` █░░░░░░░░░░░░░░░░░░░ 1/17 (16 left) — git log --graph of skewed left octopus merge.
- [ ] `t4103-apply-binary` █████░░░░░░░░░░░░░░░ 7/24 (17 left) — git apply handling binary patches

- [ ] `t4300-merge-tree` ████░░░░░░░░░░░░░░░░ 5/22 (17 left) — git merge-tree
- [ ] `t4135-apply-weird-filenames` ███░░░░░░░░░░░░░░░░░ 3/20 (17 left) — git apply with weird postimage filenames
- [ ] `t4030-diff-textconv` ██░░░░░░░░░░░░░░░░░░ 2/19 (17 left) — diff.*.textconv tests
- [ ] `t4151-am-abort` ██░░░░░░░░░░░░░░░░░░ 2/20 (18 left) — am --abort
- [ ] `t4002-diff-basic` █████████████░░░░░░░ 44/63 (19 left) — Test diff raw-output.

- [ ] `t4045-diff-relative` ██████████░░░░░░░░░░ 20/39 (19 left) — diff --relative tests
- [ ] `t4026-color` ████████░░░░░░░░░░░░ 15/34 (19 left) — Test diff/status color escape codes
- [ ] `t4027-diff-submodule` █░░░░░░░░░░░░░░░░░░░ 1/20 (19 left) — difference in submodules
- [ ] `t4038-diff-combined` ████░░░░░░░░░░░░░░░░ 6/26 (20 left) — combined diff
- [ ] `t4032-diff-inter-hunk-context` ████████░░░░░░░░░░░░ 16/37 (21 left) — diff hunk fusing
- [ ] `t4000-diff-format` ████████░░░░░░░░░░░░ 17/41 (24 left) — Test built-in diff output engine.

- [ ] `t4201-shortlog` ████░░░░░░░░░░░░░░░░ 7/32 (25 left) — 
- [ ] `t4255-am-submodule` ███░░░░░░░░░░░░░░░░░ 5/33 (28 left) — git am handling submodules
- [ ] `t4051-diff-function-context` █████░░░░░░░░░░░░░░░ 12/42 (30 left) — diff function context
- [ ] `t4200-rerere` ███░░░░░░░░░░░░░░░░░ 6/36 (30 left) — git rerere

- [ ] `t4061-diff-indent` █░░░░░░░░░░░░░░░░░░░ 2/33 (31 left) — Test diff indent heuristic.

- [ ] `t4301-merge-tree-write-tree` ░░░░░░░░░░░░░░░░░░░░ 1/33 (32 left) — git merge-tree --write-tree
- [ ] `t4209-log-pickaxe` ██████░░░░░░░░░░░░░░ 15/48 (33 left) — log --grep/--author/--regexp-ignore-case/-S/-G
- [ ] `t4068-diff-symmetric-merge-base` ░░░░░░░░░░░░░░░░░░░░ 1/36 (35 left) — behavior of diff with symmetric-diff setups and --merge-base
- [ ] `t4047-diff-dirstat` █░░░░░░░░░░░░░░░░░░░ 4/41 (37 left) — diff --dirstat tests
- [ ] `t4041-diff-submodule-option` █░░░░░░░░░░░░░░░░░░░ 4/46 (42 left) — Support for verbose submodule differences in git diff

- [ ] `t4060-diff-submodule-option-diff-format` ██░░░░░░░░░░░░░░░░░░ 6/50 (44 left) — Support for diff format verbose submodule difference in git diff

- [ ] `t4211-line-log` ██░░░░░░░░░░░░░░░░░░ 7/53 (46 left) — test log -L
- [ ] `t4020-diff-external` █████░░░░░░░░░░░░░░░ 20/72 (52 left) — external diff interface test
- [ ] `t4205-log-pretty-formats` ██████████░░░░░░░░░░ 67/125 (58 left) — Test pretty formats
- [ ] `t4034-diff-words` ██░░░░░░░░░░░░░░░░░░ 7/66 (59 left) — word diff colors
- [ ] `t4203-mailmap` ███░░░░░░░░░░░░░░░░░ 14/74 (60 left) — .mailmap configurations
- [ ] `t4015-diff-whitespace` ██████████░░░░░░░░░░ 74/136 (62 left) — Test special whitespace in diff engine.

- [ ] `t4150-am` █████░░░░░░░░░░░░░░░ 23/87 (64 left) — git am running
- [ ] `t4052-stat-output` ███░░░░░░░░░░░░░░░░░ 17/89 (72 left) — test --stat output of various commands
- [ ] `t4124-apply-ws-rule` ███░░░░░░░░░░░░░░░░░ 13/85 (72 left) — core.whitespace rules and git apply
- [ ] `t4202-log` █████████░░░░░░░░░░░ 69/149 (80 left) — git log
- [ ] `t4013-diff-various` ██████░░░░░░░░░░░░░░ 78/230 (152 left) — Various diff formatting options
- [ ] `t4216-log-bloom` █░░░░░░░░░░░░░░░░░░░ 12/167 (155 left) — git log for a path with Bloom filters
- [ ] `t4014-format-patch` ████░░░░░░░░░░░░░░░░ 47/215 (168 left) — various format-patch tests

## 6. Transport (142 files)

- [ ] `t5600-clone-fail-cleanup` ██████████████████░░ 13/14 (1 left) — test git clone to cleanup after failure

- [ ] `t5613-info-alternate` ██████████████████░░ 12/13 (1 left) — test transitive info/alternate entries
- [ ] `t5815-submodule-protos` █████████████████░░░ 7/8 (1 left) — test protocol filtering with submodules
- [ ] `t5547-push-quarantine` ████████████████░░░░ 5/6 (1 left) — check quarantine of objects during push
- [ ] `t5307-pack-missing-commit` ████████████████░░░░ 4/5 (1 left) — pack should notice missing commit objects
- [ ] `t5525-fetch-tagopt` ████████████████░░░░ 4/5 (1 left) — tagopt variable affects 
- [ ] `t5532-fetch-proxy` ████████████████░░░░ 4/5 (1 left) — fetching via git:// using core.gitproxy
- [ ] `t5330-no-lazy-fetch-with-commit-graph` ███████████████░░░░░ 3/4 (1 left) — test for no lazy fetch with the commit-graph
- [ ] `t5522-pull-symlink` ███████████████░░░░░ 3/4 (1 left) — pulling from symlinked subdir
- [ ] `t5406-remote-rejects` █████████████░░░░░░░ 2/3 (1 left) — remote push rejects are reported by client
- [ ] `t5314-pack-cycle-detection` ██████████░░░░░░░░░░ 1/2 (1 left) — test handling of inter-pack delta cycles during repack

- [ ] `t5581-http-curl-verbose` ██████████░░░░░░░░░░ 1/2 (1 left) — test GIT_CURL_VERBOSE
- [ ] `t5554-noop-fetch-negotiator` ░░░░░░░░░░░░░░░░░░░░ 0/1 (1 left) — test noop fetch negotiator
- [ ] `t5615-alternate-env` ███████████████░░░░░ 7/9 (2 left) — handling of alternates in environment variables
- [ ] `t5527-fetch-odd-refs` ████████████░░░░░░░░ 3/5 (2 left) — test fetching of oddly-named refs
- [ ] `t5306-pack-nobase` ██████████░░░░░░░░░░ 2/4 (2 left) — git-pack-object with missing base

- [ ] `t5405-send-pack-rewind` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — forced push to replace commit we do not have
- [ ] `t5524-pull-msg` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — git pull message generation
- [ ] `t5542-push-http-shallow` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — push from/to a shallow clone over http
- [ ] `t5321-pack-large-objects` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — git pack-object with 
- [ ] `t5557-http-get` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — test downloading a file by URL
- [ ] `t5565-push-multiple` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — push to group
- [ ] `t5619-clone-local-ambiguous-transport` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — test local clone with ambiguous transport
- [ ] `t5701-git-serve` █████████████████░░░ 22/25 (3 left) — test protocol v2 server commands
- [ ] `t5529-push-errors` ████████████░░░░░░░░ 5/8 (3 left) — detect some push errors early (before contacting remote)
- [ ] `t5583-push-branches` ████████████░░░░░░░░ 5/8 (3 left) — check the consisitency of behavior of --all and --branches
- [ ] `t5536-fetch-conflicts` ███████████░░░░░░░░░ 4/7 (3 left) — fetch handles conflicting refspecs correctly
- [ ] `t5308-pack-detect-duplicates` ██████████░░░░░░░░░░ 3/6 (3 left) — handling of duplicate objects in incoming packfiles
- [ ] `t5549-fetch-push-http` ░░░░░░░░░░░░░░░░░░░░ 0/3 (3 left) — fetch/push functionality using the HTTP protocol
- [ ] `t5704-protocol-violations` ░░░░░░░░░░░░░░░░░░░░ 0/3 (3 left) — Test responses to violations of the network protocol. In most

- [ ] `t5002-archive-attr-pattern` ███████████████░░░░░ 15/19 (4 left) — git archive attribute pattern tests
- [ ] `t5004-archive-corner-cases` ██████████████░░░░░░ 10/14 (4 left) — test corner cases of git-archive
- [ ] `t5351-unpack-large-objects` ████████░░░░░░░░░░░░ 3/7 (4 left) — git unpack-objects with large objects
- [ ] `t5404-tracking-branches` ████████░░░░░░░░░░░░ 3/7 (4 left) — tracking branch update checks for git push
- [ ] `t5618-alternate-refs` ██████░░░░░░░░░░░░░░ 2/6 (4 left) — test handling of --alternate-refs traversal
- [ ] `t5410-receive-pack` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — git receive-pack
- [ ] `t5517-push-mirror` ████████████░░░░░░░░ 8/13 (5 left) — pushing to a mirror repository
- [ ] `t5614-clone-submodules-shallow` ████████░░░░░░░░░░░░ 4/9 (5 left) — Test shallow cloning of repos with submodules
- [ ] `t5200-update-server-info` ███████░░░░░░░░░░░░░ 3/8 (5 left) — Test git update-server-info
- [ ] `t5564-http-proxy` ███████░░░░░░░░░░░░░ 3/8 (5 left) — test fetching through http proxy
- [ ] `t5402-post-merge-hook` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — Test the post-merge hook.
- [ ] `t5502-quickfetch` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — test quickfetch from local
- [ ] `t5544-pack-objects-hook` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — test custom script in place of pack-objects
- [ ] `t5316-pack-delta-depth` ░░░░░░░░░░░░░░░░░░░░ 0/5 (5 left) — pack-objects breaks long cross-pack delta chains
- [ ] `t5546-receive-limits` ████████████░░░░░░░░ 11/17 (6 left) — check receive input limits
- [ ] `t5534-push-signed` ██████████░░░░░░░░░░ 7/13 (6 left) — signed push
- [ ] `t5543-atomic-push` ██████████░░░░░░░░░░ 7/13 (6 left) — pushing to a repository using the atomic push option
- [ ] `t5503-tagfollow` ██████████░░░░░░░░░░ 6/12 (6 left) — test automatic tag following
- [ ] `t5408-send-pack-stdin` ████████░░░░░░░░░░░░ 4/10 (6 left) — send-pack --stdin tests
- [ ] `t5519-push-alternates` █████░░░░░░░░░░░░░░░ 2/8 (6 left) — push to a repository that borrows from elsewhere
- [ ] `t5802-connect-helper` █████░░░░░░░░░░░░░░░ 2/8 (6 left) — ext::cmd remote 
- [ ] `t5552-skipping-fetch-negotiator` ░░░░░░░░░░░░░░░░░░░░ 0/6 (6 left) — test skipping fetch negotiator
- [ ] `t5400-send-pack` ███████████░░░░░░░░░ 10/17 (7 left) — See why rewinding head breaks send-pack

- [ ] `t5523-push-upstream` ███████████░░░░░░░░░ 10/17 (7 left) — push with --set-upstream
- [ ] `t5312-prune-corruption` ███████░░░░░░░░░░░░░ 4/11 (7 left) — 

- [ ] `t5409-colorize-remote-messages` ███████░░░░░░░░░░░░░ 4/11 (7 left) — remote messages are colorized on the client
- [ ] `t5571-pre-push-hook` ███████░░░░░░░░░░░░░ 4/11 (7 left) — check pre-push hooks
- [ ] `t5313-pack-bounds-checks` ████░░░░░░░░░░░░░░░░ 2/9 (7 left) — bounds-checking of access to mmapped on-disk file formats
- [ ] `t5617-clone-submodules-remote` ████░░░░░░░░░░░░░░░░ 2/9 (7 left) — Test cloning repos with submodules using remote-tracking branches
- [ ] `t5538-push-shallow` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — push from/to a shallow clone
- [ ] `t5539-fetch-http-shallow` ██░░░░░░░░░░░░░░░░░░ 1/8 (7 left) — fetch/clone from a shallow clone over http
- [ ] `t5309-pack-delta-cycles` ░░░░░░░░░░░░░░░░░░░░ 0/7 (7 left) — test index-pack handling of delta cycles in packfiles
- [ ] `t5810-proto-disable-local` █████████████████░░░ 46/54 (8 left) — test disabling of local paths in clone/fetch
- [ ] `t5545-push-options` ███████░░░░░░░░░░░░░ 5/13 (8 left) — pushing to a repository using push options
- [ ] `t5322-pack-objects-sparse` █████░░░░░░░░░░░░░░░ 3/11 (8 left) — pack-objects object selection using sparse algorithm
- [ ] `t5620-backfill` ████░░░░░░░░░░░░░░░░ 2/10 (8 left) — git backfill on partial clones
- [ ] `t5315-pack-objects-compression` ██░░░░░░░░░░░░░░░░░░ 1/9 (8 left) — pack-object compression configuration
- [ ] `t5506-remote-groups` ██░░░░░░░░░░░░░░░░░░ 1/9 (8 left) — git remote group handling
- [ ] `t5900-repo-selection` ░░░░░░░░░░░░░░░░░░░░ 0/8 (8 left) — selecting remote repo in ambiguous cases
- [ ] `t5582-fetch-negative-refspec` ████████░░░░░░░░░░░░ 7/16 (9 left) — 
- [ ] `t5305-include-tag` ████████░░░░░░░░░░░░ 6/15 (9 left) — git pack-object --include-tag
- [ ] `t5401-update-hooks` ██████░░░░░░░░░░░░░░ 4/13 (9 left) — Test the update hook infrastructure.
- [ ] `t5621-clone-revision` █████░░░░░░░░░░░░░░░ 3/12 (9 left) — tests for git clone --revision
- [ ] `t5530-upload-pack-error` ███░░░░░░░░░░░░░░░░░ 2/11 (9 left) — errors in upload-pack
- [ ] `t5150-request-pull` ██░░░░░░░░░░░░░░░░░░ 1/10 (9 left) — Test workflows involving pull request.
- [ ] `t5320-delta-islands` ██████░░░░░░░░░░░░░░ 5/15 (10 left) — exercise delta islands
- [ ] `t5611-clone-config` ████░░░░░░░░░░░░░░░░ 3/13 (10 left) — tests for git clone -c key=value
- [ ] `t5335-compact-multi-pack-index` ░░░░░░░░░░░░░░░░░░░░ 0/10 (10 left) — multi-pack-index compaction
- [ ] `t5003-archive-zip` █████████████████░░░ 71/82 (11 left) — git archive --format=zip test
- [ ] `t5334-incremental-multi-pack-index` ██████░░░░░░░░░░░░░░ 5/16 (11 left) — incremental multi-pack-index
- [ ] `t5607-clone-bundle` ██████░░░░░░░░░░░░░░ 5/16 (11 left) — some bundle related tests
- [ ] `t5403-post-checkout-hook` ████░░░░░░░░░░░░░░░░ 3/14 (11 left) — Test the post-checkout hook.
- [ ] `t5610-clone-detached` ███░░░░░░░░░░░░░░░░░ 2/13 (11 left) — test cloning a repository with detached HEAD
- [ ] `t5605-clone-local` █████████░░░░░░░░░░░ 11/23 (12 left) — test local clone
- [ ] `t5325-reverse-index` █████░░░░░░░░░░░░░░░ 4/16 (12 left) — on-disk reverse index
- [ ] `t5560-http-backend-noserver` ██░░░░░░░░░░░░░░░░░░ 2/14 (12 left) — test git-http-backend-noserver
- [ ] `t5612-clone-refspec` █░░░░░░░░░░░░░░░░░░░ 1/13 (12 left) — test refspec written by clone-command
- [ ] `t5537-fetch-shallow` ███░░░░░░░░░░░░░░░░░ 3/16 (13 left) — fetch/clone from a shallow clone
- [ ] `t5509-fetch-push-namespaces` ██░░░░░░░░░░░░░░░░░░ 2/15 (13 left) — fetch/push involving ref namespaces
- [ ] `t5332-multi-pack-reuse` █░░░░░░░░░░░░░░░░░░░ 1/14 (13 left) — pack-objects multi-pack reuse
- [ ] `t5574-fetch-output` █░░░░░░░░░░░░░░░░░░░ 1/14 (13 left) — git fetch output format
- [ ] `t5750-bundle-uri-parse` ░░░░░░░░░░░░░░░░░░░░ 0/13 (13 left) — Test bundle-uri bundle_uri_parse_line()
- [ ] `t5303-pack-corruption-resilience` ████████████░░░░░░░░ 22/36 (14 left) — resilience to pack corruptions with redundant objects
- [ ] `t5604-clone-reference` ███████████░░░░░░░░░ 20/34 (14 left) — test clone --reference
- [ ] `t5533-push-cas` ███████░░░░░░░░░░░░░ 9/23 (14 left) — compare & swap push force/delete safety
- [ ] `t5407-post-rewrite-hook` ███░░░░░░░░░░░░░░░░░ 3/17 (14 left) — Test the post-rewrite hook.
- [ ] `t5705-session-id-in-capabilities` ██░░░░░░░░░░░░░░░░░░ 2/17 (15 left) — session ID in capabilities
- [ ] `t5814-proto-disable-ext` ████████░░░░░░░░░░░░ 11/27 (16 left) — test disabling of remote-helper paths in clone/fetch
- [ ] `t5333-pseudo-merge-bitmaps` ██░░░░░░░░░░░░░░░░░░ 2/18 (16 left) — pseudo-merge bitmaps
- [ ] `t5331-pack-objects-stdin` ░░░░░░░░░░░░░░░░░░░░ 0/16 (16 left) — pack-objects --stdin
- [ ] `t5553-set-upstream` ███░░░░░░░░░░░░░░░░░ 4/21 (17 left) — 
- [ ] `t5606-clone-options` ███░░░░░░░░░░░░░░░░░ 4/21 (17 left) — basic clone options
- [ ] `t5323-pack-redundant` █░░░░░░░░░░░░░░░░░░░ 1/18 (17 left) — Test git pack-redundant

- [ ] `t5528-push-default` ████████░░░░░░░░░░░░ 14/32 (18 left) — check various push.default settings
- [ ] `t5812-proto-disable-http` ███████░░░░░░░░░░░░░ 11/29 (18 left) — test disabling of git-over-http in clone/fetch
- [ ] `t5521-pull-options` ███░░░░░░░░░░░░░░░░░ 4/22 (18 left) — pull options
- [ ] `t5100-mailinfo` ████████████░░░░░░░░ 33/52 (19 left) — git mailinfo and git mailsplit test
- [ ] `t5541-http-push-smart` █░░░░░░░░░░░░░░░░░░░ 2/21 (19 left) — test smart pushing over http via http-backend
- [ ] `t5001-archive-attr` ██████████░░░░░░░░░░ 24/44 (20 left) — git archive attribute tests
- [ ] `t5514-fetch-multiple` ███░░░░░░░░░░░░░░░░░ 4/25 (21 left) — fetch --all works correctly
- [ ] `t5710-promisor-remote-capability` ░░░░░░░░░░░░░░░░░░░░ 1/22 (21 left) — handling of promisor remote advertisement
- [ ] `t5703-upload-pack-ref-in-want` ███░░░░░░░░░░░░░░░░░ 4/26 (22 left) — upload-pack ref-in-want
- [ ] `t5329-pack-objects-cruft` ██░░░░░░░░░░░░░░░░░░ 3/25 (22 left) — cruft pack related pack-objects tests
- [ ] `t5511-refspec` ██████████░░░░░░░░░░ 24/47 (23 left) — refspec parsing
- [ ] `t5551-http-fetch-smart` ███████░░░░░░░░░░░░░ 13/37 (24 left) — test smart fetching over http via http-backend ($HTTP_PROTO)
- [ ] `t5317-pack-objects-filter-objects` █████░░░░░░░░░░░░░░░ 9/33 (24 left) — git pack-objects using object filtering
- [ ] `t5304-prune` █████░░░░░░░░░░░░░░░ 8/32 (24 left) — prune
- [ ] `t5531-deep-submodule-push` ███░░░░░░░░░░░░░░░░░ 5/29 (24 left) — test push with submodules
- [ ] `t5548-push-porcelain` ░░░░░░░░░░░░░░░░░░░░ 1/25 (24 left) — Test git push porcelain output
- [ ] `t5512-ls-remote` ███████░░░░░░░░░░░░░ 15/40 (25 left) — git ls-remote
- [ ] `t5302-pack-index` ████░░░░░░░░░░░░░░░░ 8/36 (28 left) — pack index with 64-bit offsets and object CRC
- [ ] `t5801-remote-helpers` ██░░░░░░░░░░░░░░░░░░ 5/35 (30 left) — Test remote-helper import and export commands
- [ ] `t5616-partial-clone` ███░░░░░░░░░░░░░░░░░ 9/47 (38 left) — git partial clone
- [ ] `t5324-split-commit-graph` █░░░░░░░░░░░░░░░░░░░ 3/42 (39 left) — split commit graph
- [ ] `t5603-clone-dirname` ░░░░░░░░░░░░░░░░░░░░ 1/47 (46 left) — check output directory names used by git-clone
- [ ] `t5813-proto-disable-ssh` ███████░░░░░░░░░░░░░ 30/81 (51 left) — test disabling of git-over-ssh in clone/fetch
- [ ] `t5300-pack-object` ███░░░░░░░░░░░░░░░░░ 12/63 (51 left) — git pack-object
- [ ] `t5526-fetch-submodules` █░░░░░░░░░░░░░░░░░░░ 5/56 (51 left) — Recursive 
- [ ] `t5000-tar-tree` ████████░░░░░░░░░░░░ 36/90 (54 left) — git archive and git get-tar-commit-id test

- [ ] `t5572-pull-submodule` ██░░░░░░░░░░░░░░░░░░ 10/69 (59 left) — pull can handle submodules
- [ ] `t5515-fetch-merge-logic` ░░░░░░░░░░░░░░░░░░░░ 1/65 (64 left) — Merge logic in fetch
- [ ] `t5520-pull` ██░░░░░░░░░░░░░░░░░░ 10/80 (70 left) — pulling into void
- [ ] `t5319-multi-pack-index` ████░░░░░░░░░░░░░░░░ 24/98 (74 left) — multi-pack-indexes
- [ ] `t5318-commit-graph` ███░░░░░░░░░░░░░░░░░ 19/109 (90 left) — commit graph
- [ ] `t5601-clone` ███░░░░░░░░░░░░░░░░░ 21/115 (94 left) — 
- [ ] `t5505-remote` ██░░░░░░░░░░░░░░░░░░ 16/130 (114 left) — git remote porcelain-ish
- [ ] `t5516-fetch-push` █░░░░░░░░░░░░░░░░░░░ 7/124 (117 left) — Basic fetch/push functionality.

- [ ] `t5411-proc-receive-hook` ██████████░░░░░░░░░░ 193/354 (161 left) — Test proc-receive hook
- [ ] `t5310-pack-bitmaps` ██░░░░░░░░░░░░░░░░░░ 30/236 (206 left) — exercise basic bitmap functionality
- [ ] `t5327-multi-pack-bitmaps-rev` ░░░░░░░░░░░░░░░░░░░░ 14/314 (300 left) — exercise basic multi-pack bitmap functionality (.rev files)
- [ ] `t5326-multi-pack-bitmaps` █░░░░░░░░░░░░░░░░░░░ 29/357 (328 left) — exercise basic multi-pack bitmap functionality
- [ ] `t5500-fetch-pack` █░░░░░░░░░░░░░░░░░░░ 29/378 (349 left) — Testing multi_ack pack fetching

## 7. Rev Machinery (80 files)

- [ ] `t6100-rev-list-in-order` █████████████░░░░░░░ 2/3 (1 left) — rev-list testing in-commit-order
- [ ] `t6414-merge-rename-nocruft` █████████████░░░░░░░ 2/3 (1 left) — Merge-recursive merging renames
- [ ] `t6110-rev-list-sparse` ██████████░░░░░░░░░░ 1/2 (1 left) — operations that cull histories in unusual ways
- [ ] `t6425-merge-rename-delete` ░░░░░░░░░░░░░░░░░░░░ 0/1 (1 left) — Merge-recursive rename/delete conflict message
- [ ] `t6408-merge-up-to-date` ██████████████░░░░░░ 5/7 (2 left) — merge fast-forward and up to date
- [ ] `t6417-merge-ours-theirs` ██████████████░░░░░░ 5/7 (2 left) — Merge-recursive ours and theirs variants
- [ ] `t6114-keep-packs` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — rev-list with .keep packs
- [ ] `t6134-pathspec-in-submodule` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — test case exclude pathspec
- [ ] `t6136-pathspec-in-bare` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — diagnosing out-of-scope pathspec
- [ ] `t6413-merge-crlf` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — merge conflict in crlf repo

- [ ] `t6428-merge-conflicts-sparse` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — merge cases
- [ ] `t6431-merge-criscross` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — merge-recursive backend test
- [ ] `t6412-merge-large-rename` ██████████████░░░░░░ 7/10 (3 left) — merging with large rename matrix
- [ ] `t6400-merge-df` ███████████░░░░░░░░░ 4/7 (3 left) — Test merge with directory/file conflicts
- [ ] `t6301-for-each-ref-errors` ██████████░░░░░░░░░░ 3/6 (3 left) — for-each-ref errors for broken refs
- [ ] `t6435-merge-sparse` ██████████░░░░░░░░░░ 3/6 (3 left) — merge with sparse files
- [ ] `t6401-merge-criss-cross` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — Test criss-cross merge
- [ ] `t6421-merge-partial-clone` ░░░░░░░░░░░░░░░░░░░░ 0/3 (3 left) — limiting blob downloads when merging with partial clones
- [ ] `t6133-pathspec-rev-dwim` ██████░░░░░░░░░░░░░░ 2/6 (4 left) — test dwim of revs versus pathspecs in revision parser
- [ ] `t6404-recursive-merge` ██████░░░░░░░░░░░░░░ 2/6 (4 left) — Test merge without common ancestors
- [ ] `t6415-merge-dir-to-symlink` ███████████████░░░░░ 19/24 (5 left) — merging when a directory was replaced with a symlink
- [ ] `t6004-rev-list-path-optim` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — git rev-list trivial path optimization test

- [ ] `t6005-rev-list-count` ███░░░░░░░░░░░░░░░░░ 1/6 (5 left) — git rev-list --max-count and --skip test
- [ ] `t6439-merge-co-error-msgs` ███░░░░░░░░░░░░░░░░░ 1/6 (5 left) — unpack-trees error messages
- [ ] `t6010-merge-base` ██████████░░░░░░░░░░ 6/12 (6 left) — Merge base and parent list computation.

- [ ] `t6700-tree-depth` ████████░░░░░░░░░░░░ 4/10 (6 left) — handling of deep trees in various commands
- [ ] `t6427-diff3-conflict-markers` ██████░░░░░░░░░░░░░░ 3/9 (6 left) — recursive merge diff3 style conflict markers
- [ ] `t6060-merge-index` ██░░░░░░░░░░░░░░░░░░ 1/7 (6 left) — basic git merge-index / git-merge-one-file tests
- [ ] `t6131-pathspec-icase` ████░░░░░░░░░░░░░░░░ 2/9 (7 left) — test case insensitive pathspec limiting
- [ ] `t6102-rev-list-unexpected-objects` ████████████░░░░░░░░ 14/22 (8 left) — git rev-list should handle unexpected object types
- [ ] `t6501-freshen-objects` ███████████████░░░░░ 33/42 (9 left) — check pruning of dependent objects
- [ ] `t6433-merge-toplevel` ████████░░░░░░░░░░░░ 6/15 (9 left) — 
- [ ] `t6409-merge-subtree` █████░░░░░░░░░░░░░░░ 3/12 (9 left) — subtree merge strategy
- [ ] `t6418-merge-text-auto` ███░░░░░░░░░░░░░░░░░ 2/11 (9 left) — CRLF merge conflict across text=auto change

- [ ] `t6403-merge-file` ██████████████░░░░░░ 29/39 (10 left) — RCS merge replacement: merge-file
- [ ] `t6016-rev-list-graph-simplify-history` ███░░░░░░░░░░░░░░░░░ 2/12 (10 left) — --graph and simplified history
- [ ] `t6429-merge-sequence-rename-caching` █░░░░░░░░░░░░░░░░░░░ 1/11 (10 left) — remember regular & dir renames in sequence of merges
- [ ] `t6115-rev-list-du` ███████░░░░░░░░░░░░░ 6/17 (11 left) — basic tests of rev-list --disk-usage
- [ ] `t6001-rev-list-graft` ████░░░░░░░░░░░░░░░░ 3/14 (11 left) — Revision traversal vs grafts and path limiter
- [ ] `t6406-merge-attr` ███░░░░░░░░░░░░░░░░░ 2/13 (11 left) — per path merge controlled by merge attribute
- [ ] `t6432-merge-recursive-space-options` ░░░░░░░░░░░░░░░░░░░░ 0/11 (11 left) — merge-recursive space options

- [ ] `t6009-rev-list-parent` ████░░░░░░░░░░░░░░░░ 3/15 (12 left) — ancestor culling and limiting by parent number
- [ ] `t6426-merge-skip-unneeded-updates` █░░░░░░░░░░░░░░░░░░░ 1/13 (12 left) — merge cases
- [ ] `t6113-rev-list-bitmap-filters` █░░░░░░░░░░░░░░░░░░░ 1/14 (13 left) — rev-list combining bitmaps and filters
- [ ] `t6436-merge-overwrite` ████░░░░░░░░░░░░░░░░ 4/18 (14 left) — git-merge

- [ ] `t6003-rev-list-topo-order` ███████████░░░░░░░░░ 21/36 (15 left) — Tests git rev-list --topo-order functionality
- [ ] `t6601-path-walk` ░░░░░░░░░░░░░░░░░░░░ 0/15 (15 left) — direct path-walk API tests
- [ ] `t6437-submodule-merge` █████░░░░░░░░░░░░░░░ 6/22 (16 left) — merging with submodules
- [ ] `t6006-rev-list-format` ███████████████░░░░░ 63/80 (17 left) — git rev-list --pretty=format test
- [ ] `t6422-merge-rename-corner-cases` ██████░░░░░░░░░░░░░░ 9/26 (17 left) — recursive merge corner cases w/ renames but not criss-crosses
- [ ] `t6000-rev-list-misc` █████░░░░░░░░░░░░░░░ 6/23 (17 left) — miscellaneous rev-list tests
- [ ] `t6130-pathspec-noglob` ███░░░░░░░░░░░░░░░░░ 4/21 (17 left) — test globbing (and noglob) of pathspec limiting
- [ ] `t6424-merge-unrelated-index-changes` ██░░░░░░░░░░░░░░░░░░ 2/19 (17 left) — merges with unrelated index changes
- [ ] `t6019-rev-list-ancestry-path` █░░░░░░░░░░░░░░░░░░░ 1/18 (17 left) — --ancestry-path
- [ ] `t6101-rev-parse-parents` ██████████░░░░░░░░░░ 20/38 (18 left) — Test git rev-parse with different parent options
- [ ] `t6137-pathspec-wildcards-literal` █████░░░░░░░░░░░░░░░ 7/25 (18 left) — test wildcards and literals with git add/commit (subshell style)
- [ ] `t6411-merge-filemode` █░░░░░░░░░░░░░░░░░░░ 1/19 (18 left) — merge: handle file mode
- [ ] `t6500-gc` ████████░░░░░░░░░░░░ 15/35 (20 left) — basic git gc tests

- [ ] `t6434-merge-recursive-rename-options` ███░░░░░░░░░░░░░░░░░ 5/27 (22 left) — merge-recursive rename options

- [ ] `t6007-rev-list-cherry-pick-file` ░░░░░░░░░░░░░░░░░░░░ 1/23 (22 left) — test git rev-list --cherry-pick -- file
- [ ] `t6050-replace` ██████░░░░░░░░░░░░░░ 12/37 (25 left) — Tests replace refs functionality
- [ ] `t6200-fmt-merge-msg` █████░░░░░░░░░░░░░░░ 11/37 (26 left) — fmt-merge-msg test
- [ ] `t6430-merge-recursive` █████░░░░░░░░░░░░░░░ 10/36 (26 left) — merge-recursive backend test
- [ ] `t6416-recursive-corner-cases` ██████░░░░░░░░░░░░░░ 12/40 (28 left) — recursive merge corner cases involving criss-cross merges
- [ ] `t6017-rev-list-stdin` ████░░░░░░░░░░░░░░░░ 9/37 (28 left) — log family learns --stdin
- [ ] `t6132-pathspec-exclude` ░░░░░░░░░░░░░░░░░░░░ 1/31 (30 left) — test case exclude pathspec
- [ ] `t6135-pathspec-with-attrs` ██░░░░░░░░░░░░░░░░░░ 5/37 (32 left) — test labels in pathspecs
- [ ] `t6112-rev-list-filters-objects` ██████░░░░░░░░░░░░░░ 18/54 (36 left) — git rev-list using object filtering
- [ ] `t6022-rev-list-missing` ░░░░░░░░░░░░░░░░░░░░ 1/40 (39 left) — handling of missing objects in rev-list
- [ ] `t6600-test-reach` ██░░░░░░░░░░░░░░░░░░ 7/47 (40 left) — basic commit reachability tests
- [ ] `t6402-merge-rename` ██░░░░░░░░░░░░░░░░░░ 6/46 (40 left) — Merge-recursive merging renames
- [ ] `t6012-rev-list-simplify` ░░░░░░░░░░░░░░░░░░░░ 1/42 (41 left) — merge simplification
- [ ] `t6040-tracking-info` ░░░░░░░░░░░░░░░░░░░░ 1/44 (43 left) — remote tracking stats
- [ ] `t6002-rev-list-bisect` █░░░░░░░░░░░░░░░░░░░ 4/53 (49 left) — Tests git rev-list --bisect functionality
- [ ] `t6302-for-each-ref-filter` ███░░░░░░░░░░░░░░░░░ 12/62 (50 left) — test for-each-refs usage of ref-filter APIs
- [ ] `t6021-rev-list-exclude-hidden` ░░░░░░░░░░░░░░░░░░░░ 0/62 (62 left) — git rev-list --exclude-hidden test
- [ ] `t6018-rev-list-glob` ██████░░░░░░░░░░░░░░ 30/95 (65 left) — rev-list/rev-parse --glob
- [ ] `t6423-merge-rename-directories` ██░░░░░░░░░░░░░░░░░░ 11/82 (71 left) — recursive merge with directory renames
- [ ] `t6120-describe` █████░░░░░░░░░░░░░░░ 27/105 (78 left) — test describe
- [ ] `t6030-bisect-porcelain` ███░░░░░░░░░░░░░░░░░ 18/96 (78 left) — Tests git bisect functionality

## 8. Porcelain (94 files)

- [ ] `t7510-signed-commit` ███████████████████░ 27/28 (1 left) — signed commit tests
- [ ] `t7008-filter-branch-null-sha1` ████████████████░░░░ 5/6 (1 left) — filter-branch removal of trees with null sha1
- [ ] `t7520-ignored-hook-warning` ████████████████░░░░ 4/5 (1 left) — ignored hook warning
- [ ] `t7524-commit-summary` ██████████░░░░░░░░░░ 1/2 (1 left) — git commit summary
- [ ] `t7607-merge-state` ░░░░░░░░░░░░░░░░░░░░ 0/1 (1 left) — Test that merge state is as expected after failed merge
- [ ] `t7423-submodule-symlinks` █████████████░░░░░░░ 4/6 (2 left) — check that submodule operations do not follow symlinks
- [ ] `t7606-merge-custom` ██████████░░░░░░░░░░ 2/4 (2 left) — git merge

- [ ] `t7409-submodule-detached-work-tree` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — Test submodules on detached working tree

- [ ] `t7420-submodule-set-url` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — Test submodules set-url subcommand

- [ ] `t7514-commit-patch` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — hunk edit with 
- [ ] `t7515-status-symlinks` ██████░░░░░░░░░░░░░░ 1/3 (2 left) — git status and symlinks
- [ ] `t7516-commit-races` ░░░░░░░░░░░░░░░░░░░░ 0/2 (2 left) — git commit races
- [ ] `t7006-pager` ███████████████████░ 106/109 (3 left) — Test automatic use of a pager.
- [ ] `t7815-grep-binary` █████████████████░░░ 19/22 (3 left) — git grep in binary files
- [ ] `t7417-submodule-path-url` ████████░░░░░░░░░░░░ 2/5 (3 left) — check handling of .gitmodule path with dash
- [ ] `t7113-post-index-change-hook` █████░░░░░░░░░░░░░░░ 1/4 (3 left) — post index change hook
- [ ] `t7012-skip-worktree-writing` ████████████░░░░░░░░ 7/11 (4 left) — test worktree writing operations when skip-worktree is used
- [ ] `t7604-merge-custom-message` ██████████░░░░░░░░░░ 4/8 (4 left) — git merge

- [ ] `t7106-reset-unborn-branch` ████████░░░░░░░░░░░░ 3/7 (4 left) — git reset should work on unborn branch
- [ ] `t7615-diff-algo-with-mergy-operations` ████████░░░░░░░░░░░░ 3/7 (4 left) — git merge and other operations that rely on merge

- [ ] `t7402-submodule-rebase` ██████░░░░░░░░░░░░░░ 2/6 (4 left) — Test rebasing, stashing, etc. with submodules
- [ ] `t7421-submodule-summary-add` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — Summary support for submodules, adding them using git submodule add

- [ ] `t7518-ident-corner-cases` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — corner cases in ident strings
- [ ] `t7602-merge-octopus-many` ████░░░░░░░░░░░░░░░░ 1/5 (4 left) — git merge

- [ ] `t7103-reset-bare` ████████████░░░░░░░░ 8/13 (5 left) — git reset in a bare repository
- [ ] `t7603-merge-reduce-heads` ████████████░░░░░░░░ 8/13 (5 left) — git merge

- [ ] `t7418-submodule-sparse-gitmodules` ██████░░░░░░░░░░░░░░ 3/9 (6 left) — Test reading/writing .gitmodules when not in the working tree

- [ ] `t7701-repack-unpack-unreachable` ██████░░░░░░░░░░░░░░ 3/9 (6 left) — git repack works correctly
- [ ] `t7011-skip-worktree-reading` ██████████░░░░░░░░░░ 8/15 (7 left) — skip-worktree bit test
- [ ] `t7811-grep-open` ██████░░░░░░░░░░░░░░ 3/10 (7 left) — git grep --open-files-in-pager

- [ ] `t7419-submodule-set-branch` ████░░░░░░░░░░░░░░░░ 2/9 (7 left) — Test submodules set-branch subcommand

- [ ] `t7111-reset-table` ████████████████░░░░ 34/42 (8 left) — Tests to check that 
- [ ] `t7814-grep-recurse-submodules` ███████████████░░░░░ 26/34 (8 left) — Test grep recurse-submodules feature

- [ ] `t7517-per-repo-email` ██████████░░░░░░░░░░ 8/16 (8 left) — per-repo forced setting of email address
- [ ] `t7525-status-rename` █████████░░░░░░░░░░░ 7/15 (8 left) — git status rename detection options
- [ ] `t7412-submodule-absorbgitdirs` ██████░░░░░░░░░░░░░░ 4/12 (8 left) — Test submodule absorbgitdirs

- [ ] `t7413-submodule-is-active` ████░░░░░░░░░░░░░░░░ 2/10 (8 left) — Test with test-tool submodule is-active

- [ ] `t7817-grep-sparse-checkout` ░░░░░░░░░░░░░░░░░░░░ 0/8 (8 left) — grep in sparse checkout

- [ ] `t7611-merge-abort` ██████████░░░░░░░░░░ 10/19 (9 left) — test aborting in-progress merges

- [ ] `t7105-reset-patch` ██████░░░░░░░░░░░░░░ 4/13 (9 left) — git reset --patch
- [ ] `t7526-commit-pathspec-file` ███░░░░░░░░░░░░░░░░░ 2/11 (9 left) — commit --pathspec-from-file
- [ ] `t7007-show` ████████░░░░░░░░░░░░ 8/18 (10 left) — git show
- [ ] `t7703-repack-geometric` ████████░░░░░░░░░░░░ 8/18 (10 left) — git repack --geometric works correctly
- [ ] `t7031-verify-tag-signed-ssh` █████░░░░░░░░░░░░░░░ 4/14 (10 left) — signed tag tests
- [ ] `t7010-setup` ██████░░░░░░░░░░░░░░ 5/16 (11 left) — setup taking and sanitizing funny paths
- [ ] `t7424-submodule-mixed-ref-formats` ████░░░░░░░░░░░░░░░░ 3/14 (11 left) — submodules handle mixed ref storage formats
- [ ] `t7509-commit-authorship` █░░░░░░░░░░░░░░░░░░░ 1/12 (11 left) — commit tests of various authorhip options. 
- [ ] `t7521-ignored-mode` █░░░░░░░░░░░░░░░░░░░ 1/12 (11 left) — git status ignored modes
- [ ] `t7005-editor` ░░░░░░░░░░░░░░░░░░░░ 0/11 (11 left) — GIT_EDITOR, core.editor, and stuff
- [ ] `t7107-reset-pathspec-file` ░░░░░░░░░░░░░░░░░░░░ 0/11 (11 left) — reset --pathspec-from-file
- [ ] `t7102-reset` █████████████░░░░░░░ 26/38 (12 left) — git reset

- [ ] `t7110-reset-merge` ████████░░░░░░░░░░░░ 9/21 (12 left) — Tests for 
- [ ] `t7408-submodule-reference` █████░░░░░░░░░░░░░░░ 4/16 (12 left) — test clone --reference
- [ ] `t7426-submodule-get-default-remote` ████░░░░░░░░░░░░░░░░ 3/15 (12 left) — git submodule--helper get-default-remote
- [ ] `t7503-pre-commit-and-pre-merge-commit-hooks` ████████░░░░░░░░░░░░ 9/22 (13 left) — pre-commit and pre-merge-commit hooks
- [ ] `t7504-commit-msg-hook` ██████████░░░░░░░░░░ 16/30 (14 left) — commit-msg hook
- [ ] `t7060-wtstatus` ███░░░░░░░░░░░░░░░░░ 3/17 (14 left) — basic work tree status reporting
- [ ] `t7001-mv` ██████████████░░░░░░ 39/54 (15 left) — git mv in subdirs
- [ ] `t7416-submodule-dash-url` ███░░░░░░░░░░░░░░░░░ 3/18 (15 left) — check handling of disallowed .gitmodule urls
- [ ] `t7403-submodule-sync` ██░░░░░░░░░░░░░░░░░░ 2/18 (16 left) — git submodule sync

- [ ] `t7425-submodule-gitdir-path-extension` █████░░░░░░░░░░░░░░░ 6/23 (17 left) — submodulePathConfig extension works as expected
- [ ] `t7422-submodule-output` █░░░░░░░░░░░░░░░░░░░ 1/18 (17 left) — submodule --cached, --quiet etc. output
- [ ] `t7301-clean-interactive` ████░░░░░░░░░░░░░░░░ 5/23 (18 left) — git clean -i basic tests
- [ ] `t7505-prepare-commit-msg-hook` ████░░░░░░░░░░░░░░░░ 5/23 (18 left) — prepare-commit-msg hook
- [ ] `t7411-submodule-config` ██░░░░░░░░░░░░░░░░░░ 2/20 (18 left) — Test submodules config cache infrastructure

- [ ] `t7519-status-fsmonitor` ████████░░░░░░░░░░░░ 14/33 (19 left) — git status with file system watcher
- [ ] `t7401-submodule-summary` ████░░░░░░░░░░░░░░░░ 6/25 (19 left) — Summary support for submodules

- [ ] `t7704-repack-cruft` ████░░░░░░░░░░░░░░░░ 6/25 (19 left) — git repack works correctly
- [ ] `t7407-submodule-foreach` ██░░░░░░░░░░░░░░░░░░ 3/23 (20 left) — Test 
- [ ] `t7002-mv-sparse-checkout` ░░░░░░░░░░░░░░░░░░░░ 1/22 (21 left) — git mv in sparse working trees
- [ ] `t7601-merge-pull-config` █████████████░░░░░░░ 43/65 (22 left) — git merge

- [ ] `t7528-signed-commit-ssh` ████░░░░░░░░░░░░░░░░ 6/29 (23 left) — ssh signed commit tests
- [ ] `t7700-repack` █████████░░░░░░░░░░░ 22/47 (25 left) — git repack works correctly
- [ ] `t7061-wtstatus-ignore` ░░░░░░░░░░░░░░░░░░░░ 0/25 (25 left) — git-status ignored files
- [ ] `t7507-commit-verbose` ████████░░░░░░░░░░░░ 19/45 (26 left) — verbose commit template
- [ ] `t7064-wtstatus-pv2` ░░░░░░░░░░░░░░░░░░░░ 1/28 (27 left) — git status --porcelain=v2

- [ ] `t7450-bad-git-dotfiles` ████████░░░░░░░░░░░░ 22/50 (28 left) — check broken or malicious patterns in .git* files

- [ ] `t7600-merge` ████████████░░░░░░░░ 53/83 (30 left) — git merge

- [ ] `t7201-co` ██████░░░░░░░░░░░░░░ 15/46 (31 left) — git checkout tests.

- [ ] `t7300-clean` ████████░░░░░░░░░░░░ 22/55 (33 left) — git clean basic tests
- [ ] `t7506-status-submodule` ██░░░░░░░░░░░░░░░░░░ 4/40 (36 left) — git status for submodule
- [ ] `t7501-commit-basic-functionality` ██████████░░░░░░░░░░ 40/77 (37 left) — git commit
- [ ] `t7500-commit-template-squash-signoff` ███████░░░░░░░░░░░░░ 20/57 (37 left) — git commit

- [ ] `t7003-filter-branch` ███░░░░░░░░░░░░░░░░░ 9/48 (39 left) — git filter-branch
- [ ] `t7512-status-help` ██░░░░░░░░░░░░░░░░░░ 7/47 (40 left) — git status advice
- [ ] `t7063-status-untracked-cache` ██░░░░░░░░░░░░░░░░░░ 6/58 (52 left) — test untracked cache
- [ ] `t7508-status` ██████████░░░░░░░░░░ 66/126 (60 left) — git status
- [ ] `t7406-submodule-update` ██░░░░░░░░░░░░░░░░░░ 9/70 (61 left) — Test updating submodules

- [ ] `t7502-commit-porcelain` ████░░░░░░░░░░░░░░░░ 18/82 (64 left) — git commit porcelain-ish
- [ ] `t7900-maintenance` █░░░░░░░░░░░░░░░░░░░ 7/72 (65 left) — git maintenance builtin
- [ ] `t7400-submodule-basic` █████░░░░░░░░░░░░░░░ 37/124 (87 left) — Basic porcelain support for submodules

- [ ] `t7810-grep` █████████████░░░░░░░ 175/263 (88 left) — git grep various.

- [ ] `t7513-interpret-trailers` ██░░░░░░░░░░░░░░░░░░ 11/99 (88 left) — git interpret-trailers
- [ ] `t7004-tag` ████████████░░░░░░░░ 139/231 (92 left) — git tag


## 9. Misc (12 files)

- [ ] `t8008-blame-formats` ████████████████░░░░ 4/5 (1 left) — blame output in various formats on a simple case
- [ ] `t8004-blame-with-conflicts` █████████████░░░░░░░ 2/3 (1 left) — git blame on conflicted files
- [ ] `t8009-blame-vs-topicbranches` ██████████░░░░░░░░░░ 1/2 (1 left) — blaming through history with topic branches
- [ ] `t8010-cat-file-filters` ████████░░░░░░░░░░░░ 4/9 (5 left) — git cat-file filters support
- [ ] `t8015-blame-diff-algorithm` █████░░░░░░░░░░░░░░░ 2/7 (5 left) — git blame with specific diff algorithm
- [ ] `t8007-cat-file-textconv` ████████████░░░░░░░░ 9/15 (6 left) — git cat-file textconv support
- [ ] `t8011-blame-split-file` ████░░░░░░░░░░░░░░░░ 2/10 (8 left) — 

- [ ] `t8006-blame-textconv` ███████░░░░░░░░░░░░░ 6/16 (10 left) — git blame textconv support
- [ ] `t8014-blame-ignore-fuzzy` █████░░░░░░░░░░░░░░░ 4/16 (12 left) — git blame ignore fuzzy heuristic
- [ ] `t8013-blame-ignore-revs` ███░░░░░░░░░░░░░░░░░ 3/19 (16 left) — ignore revisions when blaming
- [ ] `t8003-blame-corner-cases` ████████░░░░░░░░░░░░ 12/30 (18 left) — git blame corner cases
- [ ] `t8020-last-modified` ░░░░░░░░░░░░░░░░░░░░ 1/28 (27 left) — last-modified tests

## 10. Contrib/Other (15 files)

- [x] `t9304-fast-import-marks` ████████████████████ 8/8 (0 left) — test exotic situations with marks
- [x] `t9850-shell` ████████████████████ 5/5 (0 left) — git shell tests
- [x] `t9305-fast-import-signatures` ████████████████████ 21/21 (0 left) — git fast-import --signed-commits=<mode>
- [x] `t9306-fast-import-signed-tags` ████████████████████ 10/10 (0 left) — git fast-import --signed-tags=<mode>
- [x] `t9351-fast-export-anonymize` ████████████████████ 17/17 (0 left) — basic tests for fast-export --anonymize
- [x] `t9210-scalar` ████████████████████ 22/22 (0 left) — test the `scalar` command
- [x] `t9301-fast-import-notes` ████████████████████ 17/17 (0 left) — test git fast-import of notes objects
- [x] `t9003-help-autocorrect` ████████████████████ 10/10 (0 left) — help.autocorrect finding a match
- [x] `t9002-column` ████████████████████ 16/16 (0 left) — git column
- [x] `t9211-scalar-clone` ████████████████████ 14/14 (0 left) — test the `scalar clone` subcommand
- [x] `t9303-fast-import-compression` ████████████████████ 16/16 (0 left) — compression setting of fast-import utility
- [x] `t9350-fast-export` ████████████████████ 73/73 (0 left) — git fast-export
- [x] `t9903-bash-prompt` ████████████████████ 67/67 (0 left) — test git-specific bash prompt functions
- [x] `t9001-send-email` ████████████████████ 216/216 (0 left) — git send-email
- [x] `t9902-completion` ████████████████████ 263/263 (0 left) — test bash completion

**Total: 765 tracked files**
**8,690/24,806 tests passing, 16,116 failures remaining**
