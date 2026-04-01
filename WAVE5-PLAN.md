# Wave 5 Plan — March Toward 100%

**Starting point:** 899 tests passing, 729 ported (of 19,066 upstream), ~3.8% coverage, 56 commands

## Strategy

Two-pronged attack:
1. **Deepen existing commands** — massive gains in files we've already started
2. **New test files** — port entirely new test files for already-implemented commands
3. **New commands** — implement commands that unlock large test file clusters

## Phase 1: Deepen Existing (Biggest Gaps)

Priority order by remaining tests:

| File | Ported | Upstream | Remaining | Command |
|------|--------|----------|-----------|---------|
| t1400-update-ref.sh | 10 | 254 | 244 | update-ref |
| t7004-tag.sh | 30 | 219 | 189 | tag |
| t1300-config.sh | 64 | 237 | 173 | config |
| t3200-branch.sh | 9 | 167 | 158 | branch |
| t0000-basic.sh | 6 | 139 | 133 | basic |
| t4202-log.sh | 12 | 145 | 133 | log |
| t7508-status.sh | 9 | 122 | 113 | status |
| t0001-init.sh | 32 | 94 | 62 | init |
| t0008-ignores.sh | 12 | 82 | 70 | check-ignore |
| t1006-cat-file.sh | 51 | 121 | 70 | cat-file |
| t3600-rm.sh | 23 | 82 | 59 | rm |
| t3700-add.sh | 12 | 58 | 46 | add |
| t6302-for-each-ref-filter.sh | 7 | 51 | 44 | for-each-ref |
| t7700-repack.sh | 4 | 47 | 43 | repack |
| t1000-read-tree-m-3way.sh | 3 | 39 | 36 | read-tree |
| t7001-mv.sh | 20 | 54 | 34 | mv |
| t4013-diff-various.sh | 4 | 37 | 33 | diff |
| t6006-rev-list-format.sh | 4 | 37 | 33 | rev-list |
| t1404-update-ref-errors.sh | 7 | 38 | 31 | update-ref |
| t5302-pack-index.sh | 5 | 36 | 31 | show-index/verify-pack |

Subtotal remaining: ~1,662 tests

## Phase 2: New Test Files (Implemented Commands)

| File | Tests | Command |
|------|-------|---------|
| t6018-rev-list-glob.sh | 69 | rev-list |
| t7500-commit-template-squash-signoff.sh | 53 | commit |
| t7063-status-untracked-cache.sh | 58 | status |
| t4047-diff-dirstat.sh | 41 | diff |
| t0027-auto-crlf.sh | 39 | config/add |
| t1502-rev-parse-parseopt.sh | 35 | rev-parse |
| t4026-color.sh | 34 | config/diff |
| t1305-config-include.sh | 37 | config |
| t1308-config-set.sh | 37 | config |
| t3201-branch-contains.sh | 24 | branch |
| t3203-branch-output.sh | 41 | branch |
| t2200-add-update.sh | 19 | add |
| t2203-add-intent.sh | 19 | add |
| t2060-switch.sh | 16 | switch |
| t2018-checkout-branch.sh | 25 | checkout |
| t2020-checkout-detach.sh | 25 | checkout |
| t1303-wacky-config.sh | 11 | config |
| t1307-config-blob.sh | 13 | config |

Subtotal: ~596 tests

## Phase 3: New Commands

Commands that unlock the most test coverage:
- **apply** — unlocks t4100-t4124 (~200+ tests), critical for am/rebase
- **clone** — unlocks t5601 (90 tests), many other files use clone
- **merge** — unlocks t7600 (83 tests), required by many other tests
- **pack-objects/index-pack** — unlocks t5300 (50+ tests), critical for network ops
- **grep** — unlocks t7810 (176 tests)
- **fsck** — unlocks t1450 (74 tests)
- **describe** — unlocks t6120 (but we have name-rev already)
- **clean** — small command, unlocks several cherry-pick test files
- **shortlog** — unlocks t4201
- **reflog** — unlocks t1410-t1421 (~80 tests)
- **notes** — unlocks t3301 (125 tests)
- **check-attr** — unlocks t0003 (60+ tests)
- **version** — trivial

## Execution Order

Batch A (parallel): deepen update-ref, tag, config, branch, log
Batch B (parallel): deepen basic, status, init, ignores, cat-file
Batch C (parallel): new files — rev-list-glob, commit-template, status-untracked, diff-dirstat
Batch D: new commands — apply, clone, merge, pack-objects
Batch E (parallel): deepen rm, add, for-each-ref, repack, read-tree, mv
Batch F: more new files + more new commands
... continue until 100% or interrupted
