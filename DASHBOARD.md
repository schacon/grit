# Grit Test Dashboard

## Test Results

**5,527 / 16,705 upstream test cases ported (33%)**

- 195 / 888 upstream test files ported
- 14,153 additional grit-specific tests
- 99.9% pass rate across all ported tests

### Coverage by Area

| Area | Files | Tests | Ported | Ported Tests | % |
|------|------:|------:|-------:|-------------:|--:|
| Basic/Setup (t0xxx) | 80 | 1,415 | 16 | 647 | 45% |
| Plumbing: read-tree, cat-file, refs (t1xxx) | 94 | 2,144 | 41 | 1,310 | 61% |
| Checkout/Index (t2xxx) | 63 | 761 | 22 | 257 | 33% |
| ls-files, ls-tree, merge, cherry-pick, rm, add, mv (t3xxx) | 126 | 2,379 | 24 | 603 | 25% |
| Diff (t4xxx) | 148 | 2,432 | 38 | 945 | 38% |
| Pack/Fetch/Push/Clone (t5xxx) | 172 | 3,049 | 6 | 188 | 6% |
| Rev-list, rev-parse, merge-base, for-each-ref (t6xxx) | 92 | 1,596 | 25 | 544 | 34% |
| Porcelain: commit, status, tag, branch, reset (t7xxx) | 113 | 2,929 | 23 | 1,033 | 35% |
| **Total** | **888** | **16,705** | **195** | **5,527** | **33%** |

### Largest Uncovered Upstream Test Files

| File | Tests | Description |
|------|------:|-------------|
| `t9902-completion.sh` | 261 | test bash completion |
| `t4014-format-patch.sh` | 215 | various format-patch tests |
| `t9300-fast-import.sh` | 204 | test git fast-import utility |
| `t9001-send-email.sh` | 200 | git send-email |
| `t7810-grep.sh` | 176 | git grep various.
 |
| `t3903-stash.sh` | 142 | Test git stash |
| `t3404-rebase-interactive.sh` | 132 | git rebase interactive
 |
| `t3301-notes.sh` | 125 | Test commit notes |
| `t5505-remote.sh` | 125 | git remote porcelain-ish |
| `t7400-submodule-basic.sh` | 123 | Basic porcelain support for submodules
 |
| `t9500-gitweb-standalone-no-errors.sh` | 120 | gitweb as standalone script (basic tests).
 |
| `t3701-add-interactive.sh` | 111 | add -i basic tests |
| `t5516-fetch-push.sh` | 111 | Basic fetch/push functionality.
 |
| `t5510-fetch.sh` | 106 | Per branch config variables affects  |
| `t1092-sparse-checkout-compatibility.sh` | 104 | compare full workdir to sparse workdir |

Run `bash tests/harness/run-all-count.sh` to verify pass rates.
