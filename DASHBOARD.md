# Grit Test Dashboard

## Test Results

**15,897 / 16,705 upstream test cases ported (95%)**

- 826 / 888 upstream test files ported
- 14,240 additional grit-specific tests
- 99.9% pass rate across all ported tests

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

### Largest Uncovered Upstream Test Files

| File | Tests | Description |
|------|------:|-------------|
| `t9902-completion.sh` | 261 | test bash completion |
| `t9300-fast-import.sh` | 204 | test git fast-import utility |
| `t9001-send-email.sh` | 200 | git send-email |
| `t9500-gitweb-standalone-no-errors.sh` | 120 | gitweb as standalone script (basic tests).
 |
| `t5702-protocol-v2.sh` | 85 | test git wire-protocol version 2 |
| `t9350-fast-export.sh` | 73 | git fast-export |
| `t9903-bash-prompt.sh` | 67 | test git-specific bash prompt functions |
| `t5550-http-fetch-dumb.sh` | 57 | test dumb fetching over http via static file |
| `t9809-git-p4-client-view.sh` | 57 | git p4 client view |
| `t5551-http-fetch-smart.sh` | 55 | test smart fetching over http via http-backend ($HTTP_PROTO) |
| `t9400-git-cvsserver-server.sh` | 45 | git-cvsserver access
 |
| `t9119-git-svn-info.sh` | 41 | git svn info |
| `t5541-http-push-smart.sh` | 37 | test smart pushing over http via http-backend |
| `t5558-clone-bundle-uri.sh` | 37 | test fetching bundles with --bundle-uri |
| `t9402-git-cvsserver-refs.sh` | 37 | git-cvsserver and git refspecs
 |

Run `bash tests/harness/run-all-count.sh` to verify pass rates.
