## Task: t4039-diff-assume-unchanged

### Claim
- Claimed after completing `t4133-apply-filenames`, before continuing `t4049-diff-stat-count`.
- Marked as `[~]` in `PLAN.md` during implementation, then `[x]` after validation.

### Baseline
- `./scripts/run-tests.sh t4039-diff-assume-unchanged.sh` initially reported `2/4` passing.
- Direct repro:
  - `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4039-diff-assume-unchanged.sh`
  - failures:
    - setup failure because `git ls-files -v one` rejected `-v`
    - `diff-files does not examine assume-unchanged entries`

### Root cause
- `grit ls-files` did not parse the `-v` flag used by this test for lowercase tag display.
- `grit diff-files` still considered entries marked `assume-unchanged`, while the test expects them to be ignored.

### Fix implemented
- Updated `grit/src/commands/ls_files.rs`:
  - added `-v` (`show_untracked_cache_tag`) parsing.
  - made `-v` tag rendering lowercase (e.g. `h one`) while reusing existing tag pipeline.
- Updated `grit/src/commands/diff_files.rs`:
  - skipped index entries with `assume_unchanged()` in `collect_changes`.

### Validation
- `cargo build --release` -> pass.
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash tests/t4039-diff-assume-unchanged.sh` -> `4/4` pass.
- `./scripts/run-tests.sh t4039-diff-assume-unchanged.sh` -> `4/4` pass.
- Regression checks:
  - `./scripts/run-tests.sh t4133-apply-filenames.sh` -> `4/4` pass.
  - `./scripts/run-tests.sh t4117-apply-reject.sh` -> `8/8` pass.
  - `./scripts/run-tests.sh t4112-apply-renames.sh` -> `2/2` pass.
  - `./scripts/run-tests.sh t4131-apply-fake-ancestor.sh` -> `3/3` pass.
  - `./scripts/run-tests.sh t4125-apply-ws-fuzz.sh` -> `4/4` pass.
