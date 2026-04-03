# Grit Test Dashboard

## Test Results

**27,724 / 27,994 individual test cases passing (99.0%)**

- 1,375 test files (upstream ports + grit-specific)
- 23,539 test_expect_success all passing (0 unexpected failures)
- 272 test_expect_failure remaining (known gaps needing grit code fixes)
- 267 skipped (missing prereqs)

*Last update: 2026-04-03 13:42 UTC*

### Recent Changes
- diff: `--stat=<width>[,<name-width>[,<count>]]` — 3 tests flipped
- diff: `A...B` symmetric diff, `A..B` range — 5 tests flipped
- Total: 8 test_expect_failure → test_expect_success since last recount

### Coverage by Area

| Area | Files | Tests | Ported | Ported Tests | % |
|------|------:|------:|-------:|-------------:|--:|
| Basic/Setup (t0xxx) | 80 | 1,415 | 80 | 1,415 | 100% |
| Plumbing: read-tree, cat-file, refs (t1xxx) | 94 | 2,144 | 94 | 2,144 | 100% |
| Checkout/Index (t2xxx) | 63 | 761 | 63 | 761 | 100% |
| ls-files, ls-tree, merge, cherry-pick, rm, add, mv (t3xxx) | 126 | 2,379 | 126 | 2,379 | 100% |
| Diff (t4xxx) | 148 | 2,432 | 148 | 2,432 | 100% |
| Pack/Fetch/Push/Clone (t5xxx) | 172 | 3,049 | 111 | 2,256 | 73% |
| Rev-list, rev-parse, merge-base, for-each-ref (t6xxx) | 92 | 1,596 | 92 | 1,596 | 100% |
| Porcelain: commit, status, tag, branch, reset (t7xxx) | 113 | 2,929 | 112 | 2,914 | 99% |
| **Total** | **888** | **16,705** | **826** | **15,897** | **95%** |

### Remaining test_expect_failure (272 total)

| File | Count | Area |
|------|------:|------|
| t4150-am.sh | 44 | `am` command |
| t1307-config-blob.sh | 15 | `config --blob` |
| t6112-rev-list-filters-objects.sh | 9 | rev-list object filters |
| t4065-diff-anchored.sh | 5 | diff anchored algorithm |
| t7065-status-rename.sh | 5 | status rename detection |
| t0020-crlf.sh | 5 | CRLF handling |
| t7414-submodule-mistakes.sh | 4 | submodule edge cases |
| t4004-diff-rename-symlink.sh | 4 | diff rename + symlinks |
| (170 more across 80+ files) | 181 | various |

Run `bash tests/harness/run-all-count.sh` to verify pass rates.
