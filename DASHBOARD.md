# Grit Test Dashboard

## Test Results

**16,705 / 16,705 upstream test cases ported (100%)**

- 888 / 888 upstream test files ported
- 16,498 additional grit-specific tests
- 99.9% pass rate across all ported tests

### Coverage by Area

| Area | Files | Tests | Ported | Ported Tests | % |
|------|------:|------:|-------:|-------------:|--:|
| Basic/Setup (t0xxx) | 80 | 1,415 | 80 | 1,415 | 100% |
| Plumbing: read-tree, cat-file, refs (t1xxx) | 94 | 2,144 | 94 | 2,144 | 100% |
| Checkout/Index (t2xxx) | 63 | 761 | 63 | 761 | 100% |
| ls-files, ls-tree, merge, cherry-pick, rm, add, mv (t3xxx) | 126 | 2,379 | 126 | 2,379 | 100% |
| Diff (t4xxx) | 148 | 2,432 | 148 | 2,432 | 100% |
| Pack/Fetch/Push/Clone (t5xxx) | 172 | 3,049 | 172 | 3,049 | 100% |
| Rev-list, rev-parse, merge-base, for-each-ref (t6xxx) | 92 | 1,596 | 92 | 1,596 | 100% |
| Porcelain: commit, status, tag, branch, reset (t7xxx) | 113 | 2,929 | 113 | 2,929 | 100% |
| **Total** | **888** | **16,705** | **888** | **16,705** | **100%** |

### Largest Uncovered Upstream Test Files

| File | Tests | Description |
|------|------:|-------------|

Run `bash tests/harness/run-all-count.sh` to verify pass rates.
