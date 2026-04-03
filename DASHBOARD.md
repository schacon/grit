# Grit Test Dashboard

## Test Results

**27,776 / 28,007 individual test cases passing (99.2%)**

- 1,375 test files (upstream ports + grit-specific)
- 27,776 test_expect_success all passing (0 unexpected failures)
- 231 test_expect_failure remaining (known gaps needing grit code fixes)
- ~267 skipped (missing prereqs)

*Last update: 2026-04-03 14:06 UTC*

### Recent Changes
- am: hook execution + --continue rejects unchanged index — 10 tests flipped
- diff: `--relative[=<path>]` path prefix stripping — 7 tests flipped
- status: rename detection via deleted+added entries — 5 tests flipped
- rev-list: `--filter` (blob:none, blob:limit, tree:N, combine) — 9 tests flipped
- config: `--blob=<blob-ish>` — 15 tests flipped
- ls-files: -t status tag, -m modified, -d deleted — 5 tests flipped
- ls-files: --exclude, --exclude-standard, --directory — 2 tests flipped
- Total: 231 test_expect_failure remaining (down from 278)

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

### Remaining test_expect_failure (231 total)

| File | Count | Area |
|------|------:|------|
| t4150-am.sh | 34 | `am` command |
| t4065-diff-anchored.sh | 5 | diff anchored algorithm |
| t0020-crlf.sh | 5 | CRLF handling |
| t7414-submodule-mistakes.sh | 4 | submodule edge cases |
| t4004-diff-rename-symlink.sh | 4 | diff rename + symlinks |
| t4003/4005/4009-diff-rename | 9 | diff rename detection |
| t5621-clone-revision.sh | 3 | clone --revision |
| t0610/0613-reftable | 6 | reftable backend |
| (remaining across 70+ files) | 161 | various |

Run `bash tests/harness/run-all-count.sh` to verify pass rates.
