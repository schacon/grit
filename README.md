# Grit — Git in Rust

Grit is a **from-scratch reimplementation of Git** in idiomatic Rust. The goal is to match Git's behavior closely enough that the upstream test suite (under `git/t/`) can be ported and run against this tool.

This implementation is being written entirely by AI coding agents. The AGENT.md instructions and a snapshot of the Git source code were provided, and autonomous agents (first Cursor, then OpenClaw orchestrating Claude Code) implement commands, port tests, and validate against the upstream Git test suite.

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

Run `python3 scripts/build-dashboard.py` to regenerate, or `bash tests/harness/run-all-count.sh` to verify pass rates.


