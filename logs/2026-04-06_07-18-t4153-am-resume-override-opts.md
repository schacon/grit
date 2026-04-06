## Task: t4153-am-resume-override-opts

### Claim
- Claimed from the plan after completing `t4140`.
- Baseline before implementation:
  - local harness: `1/6`
  - upstream harness: `1/6`

### Baseline failures
- `--retry` unsupported/incomplete behavior for non-in-progress sessions and retries.
- resume option override behavior missing for:
  - `--3way` over saved `--no-3way`
  - `--no-quiet` over saved `--quiet`
  - `--signoff` over saved `--no-signoff`
  - `--reject` over saved `--no-reject`
- Underlying patch fixture generation for `format-patch --stdout -1 <rev>` selected the wrong commit in this scenario (`side2` instead of `side1`), breaking the intended resume flow.

### Implemented fixes
1. **`grit am` resume/retry override plumbing**
   - Added CLI support:
     - `--retry`
     - `--no-quiet`
     - `--no-signoff`
     - `--reject`
     - `--no-reject`
   - Added option override model (`AmOptionOverrides`) and merge helpers to apply overrides only to the resumed patch as expected by upstream semantics.
   - Added `do_retry` path:
     - errors with `operation not in progress` when no am session exists,
     - reuses in-progress session otherwise with first-patch override application.

2. **Persist/restore reject option in AM session state**
   - `save_am_options` now records `reject`.
   - `load_am_options` now restores `reject`.

3. **`format-patch -1 <rev>` exact-commit behavior**
   - Fixed `format-patch` so `-1` with an explicit revision emits that exact commit, not the commits *after* it.
   - Added helper `collect_single_commit`.
   - This restores correct fixture generation in `t4153` setup (`side1.eml` contains `side1` patch).

4. **Three-way retry behavior for rename case**
   - Improved `am` three-way merge fallback:
     - parse and retain `index <old>..<new>` preimage oid from patch headers,
     - use that old blob as authoritative preimage/base when available,
     - preserve rename/content-match fallback when patch path is missing in worktree.
   - This allows `--retry --3way` to apply `side1` onto renamed `file2`, then fail at `side2`, matching upstream expectations.

5. **Reject-file behavior on resume with `--reject`**
   - Added reject artifact writing for failing patch application in am flow.
   - `--retry --reject` now creates `<path>.rej` while leaving session in progress and failing command as expected.

### Validation
- `cargo build --release` ✅
- `TEST_VERBOSE=1 EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4153-am-resume-override-opts.sh` (from `tests/`) ✅ `6/6`
- `./scripts/run-tests.sh t4153-am-resume-override-opts.sh` ✅ `6/6`
- `bash scripts/run-upstream-tests.sh t4153-am-resume-override-opts` ✅ `6/6`

### Regression/quality checks
- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty` ✅ (reverted unrelated autofix churn outside task scope)
- `cargo test -p grit-lib --lib` ✅
- `./scripts/run-tests.sh t4140-apply-ita.sh` ✅ `7/7`
- `./scripts/run-tests.sh t4116-apply-reverse.sh` ✅ `7/7`

