# t1505-rev-parse-last — @{-N} syntax

**Date:** 2026-04-05
**Branch:** fix/rev-parse-last
**Result:** 7/7 tests pass

## Summary

The t1505-rev-parse-last test file tests the `@{-N}` syntax for referring to previously checked-out branches.

Tests:
1. setup — creates branches and merge history
2. `@{-1}` works — resolves to the last branch
3. `@{-1}~2` works — ancestor traversal on last branch
4. `@{-1}^2` works — second parent of last branch
5. `@{-1}@{1}` works — compound reflog syntax on last branch
6. `@{-2}` works — second-to-last branch
7. `@{-3}` fails — correctly errors when N exceeds history

## Findings

All 7 tests were already passing on this branch. The fix for the compound `@{-N}@{M}` reflog syntax was included in commit `582663af` ("fix: make 10 plumbing test files fully pass"). No additional code changes were needed.

## Actions Taken

- Confirmed all 7/7 tests pass
- Marked `t1505-rev-parse-last` as `[x]` in plan.md
- Created progress.md
