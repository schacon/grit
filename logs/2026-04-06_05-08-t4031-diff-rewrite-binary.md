## Task: t4031-diff-rewrite-binary

### Claim
- Claimed from `PLAN.md` after completing `t4044`.
- Initial status:
  - local harness: `3/8`
  - upstream harness: `3/8`

### Baseline failures
- Failing tests: 3, 4, 5, 6, 8.
- Upstream failure details (`/tmp/grit-upstream-results/t4031-diff-rewrite-binary.out`):
  - `git diff -B` missing `dissimilarity index` metadata.
  - `git diff -B --binary` missing rewrite metadata / patch shape parity.
  - `git diff -B --numstat --summary` missing binary `-\t-\t` numstat row and `rewrite` summary.
  - `git diff -B --stat --summary` missing expected rewrite summary line.
  - `git diff -B` with textconv missing converted minus/plus hexdump lines.

### Native-vs-grit observed deltas
- Native Git (`git diff -B`) emits:
  - `dissimilarity index 99%`
  - `index <old>..<new> 100644`
  - `Binary files a/file and b/file differ`
- Current Grit emits:
  - `index <old>..<new> 100644`
  - `Binary files a/file and b/file differ`
  - no `dissimilarity index`.
- Native Git numstat for rewrite binary:
  - `-\t-\tfile`
  - ` rewrite file (99%)`
- Current Grit numstat:
  - `450\t450\tfile`
  - no rewrite summary.

### Current implementation notes
- `diff.rs` currently treats `-B/--break-rewrites` as parser-accepted but does not convert
  modified entries into rewrite metadata for patch/stat/summary/numstat paths.
- Textconv metadata exists in config parsing (`diff.<driver>.textconv`) but `diff` does not yet
  apply textconv conversion for patch rendering in binary rewrite mode.

### Next implementation plan (in progress)
1. Add rewrite dissimilarity detection for modified binary/textconv entries under `-B`.
2. Emit `dissimilarity index <N>%` in patch headers for rewrite entries.
3. Emit `rewrite ... (<N>%)` summary lines under `--summary`.
4. Emit `-\t-\t<path>` for binary rewrite under `--numstat`.
5. Add textconv application for patch output so rewrite hunks can show converted lines.

### Implementation completed
- Implemented `test-tool hexdump` in `grit/src/main.rs` and wired it through the
  `test-tool` dispatcher.
- Extended `.gitattributes` parsing in `grit-lib/src/crlf.rs`:
  - `FileAttrs` now captures `diff_driver` from `diff=<driver>` attributes.
- Exported `rename_similarity_score` from `grit-lib/src/diff.rs` to reuse rename
  scoring logic for rewrite dissimilarity metadata.
- Updated `grit/src/commands/diff.rs`:
  - Added rewrite dissimilarity computation for `-B`.
  - Added textconv resolution/application pipeline for binary paths using
    `.gitattributes` + `diff.<driver>.textconv`.
  - Patch output now emits `dissimilarity index` and textconv hunks for binary rewrites.
  - `--numstat` now prints `-\t-\t` for binary changes and appends rewrite summary
    when `--summary` is also requested.
  - `--stat` binary rows retain `Bin` and include size transition suffix while summary
    totals remain `0 insertions(+), 0 deletions(-)` for binary rewrites.
  - Summary output now emits `rewrite <path> (<N>%)` for modified entries when `-B` is set.

### Validation
- `cargo build --release` — pass
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4031-diff-rewrite-binary.sh` (from `tests/`) — `8/8` pass
- `./scripts/run-tests.sh t4031-diff-rewrite-binary.sh` — `8/8` pass; `data/file-results.tsv` updated
- `bash scripts/run-upstream-tests.sh t4031-diff-rewrite-binary` — `8/8` pass
- Quality gates:
  - `cargo fmt` — pass
  - `cargo clippy --fix --allow-dirty` — pass (reverted unrelated autofixes outside task scope)
  - `cargo test -p grit-lib --lib` — pass (96 tests)
