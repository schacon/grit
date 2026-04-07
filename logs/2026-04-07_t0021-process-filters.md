# t0021 conversion progress (2026-04-07)

## Done this session

- Fixed `test-tool genrandom`: match Git when size omitted (pipe-driven length); `tests/test-tool` delegates to `grit test-tool genrandom` (fixes empty `*.file` in t0021 test 21).
- Filter protocol: record `delay` cap; no-op process smudge when server lacks `smudge` (test 20).
- Merge: checkout after merge commit; `treeish=` from merged branch tip; octopus checkout after commit with commit OID.
- Reset `--hard`: `smudge_meta_for_reset` + extended `checkout_index_to_worktree` for `ref=` on symbolic specs (test 18 `old-main`).
- Harness: t0021 now **32/42** passing (`./scripts/run-tests.sh t0021-conversion.sh`).

## Push

`git push -u origin cursor/t0021-conversion-test-passing-2936` failed: remote `origin` URL not a usable repo in this environment.

## Remaining t0021

Filter restart on write failure, error/abort paths, invalid filter, full delayed checkout + progress messages (~10 tests).
