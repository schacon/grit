## Task: t4122-apply-symlink-inside

### Claim
- Claimed after completing `t4010-diff-pathspec`.
- Marked as `[~]` in `PLAN.md`.

### Baseline
- Tracked harness:
  - `./scripts/run-tests.sh t4122-apply-symlink-inside.sh`
  - result: **1/7 passing** (6 failing)
- Direct local run from `tests/`:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4122-apply-symlink-inside.sh`
  - result: **1/7 passing** (same 6 failing tests)

### Current failing assertions
- 1 `setup`
- 2 `apply`
- 3 `check result`
- 4 `do not read from beyond symbolic link`
- 6 `do not follow symbolic link (same input)`
- 7 `do not follow symbolic link (existing)`

### Initial observations
- Setup currently fails before later assertions due `git diff --binary ...` incompatibility in this path (`unknown revision '--binary'`), so `to-apply.patch` is not generated and dependent checks fail.
- Symlink traversal hardening checks (tests 4, 6, 7) indicate `apply` path handling currently allows or mis-handles writes through symlinked directory segments.
- Need to inspect `apply` option parsing for `--binary` in diff generation and enforce safe path resolution that rejects/guards symlink hops in patch targets.
