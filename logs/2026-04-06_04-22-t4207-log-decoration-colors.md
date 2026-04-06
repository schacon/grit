## Task: t4207-log-decoration-colors

### Claim
- Claimed from `PLAN.md` after completing `t4074-diff-shifted-matched-group`.
- Initial local harness status: `1/4` passing.

### Baseline and investigation
- `./scripts/run-tests.sh t4207-log-decoration-colors.sh` reported `1/4` in this repo mirror.
- Direct test execution from `tests/` reproduced the same local result and showed color decode mismatches for tag decorations.
- Upstream authoritative run:
  - `bash scripts/run-upstream-tests.sh t4207-log-decoration-colors` reported `4/4`.
- Local failure root cause was isolated to this simplified mirror’s `tests/test-lib.sh` helper:
  - `test_decode_color` maps only split single-code ANSI escapes (e.g., `\x1b[1m`, `\x1b[7m`, `\x1b[33m`);
  - it strips combined ANSI sequences like `\x1b[1;7;33m`, causing expected `<BOLD;REVERSE;YELLOW>` comparisons to fail despite correct raw output.

### Implemented behavior in code (already in working tree)
- `grit/src/commands/log.rs`:
  - Added decoration kind modeling and deterministic decoration rendering with color-aware formatting.
  - Implemented `color.decorate.*` mapping for branch/remote/tag/stash/HEAD/grafted categories.
  - Ensured tag decorations keep multi-attribute color styles across both prefix and label text.
  - Added decoration ordering and formatting logic for `HEAD`, local/remote refs, tags, stash, and grafted markers.
- `grit/src/commands/replace.rs` and `grit-lib/src/repo.rs`:
  - Added `GIT_REPLACE_REF_BASE` support for replace-ref creation/lookup/listing and replaced-object decoration discovery.
- `grit/src/commands/stash.rs`:
  - Aligned stash identity/timestamp resolution with test tick environment behavior to keep deterministic ordering in decoration output.

### Validation
- `cargo build --release` ✅
- `./scripts/run-tests.sh t4207-log-decoration-colors.sh` ✅/⚠️ local mirror reports `1/4` (known helper decoding limitation)
- `EDITOR=: VISUAL=: LC_ALL=C LANG=C GUST_BIN="/workspace/target/release/grit" bash t4207-log-decoration-colors.sh` (from `tests/`) ✅/⚠️ `1/4` with same known local decoder mismatch
- `bash scripts/run-upstream-tests.sh t4207-log-decoration-colors` ✅ `4/4` (authoritative pass)
- Raw ANSI verification in local output confirmed expected combined tag escape sequences (`\x1b[1;7;33m`) are present.
- Quality gates:
  - `cargo fmt` ✅
  - `cargo clippy --fix --allow-dirty` ✅ (unrelated autofixes reverted outside scope)
  - `cargo test -p grit-lib --lib` ✅

### Outcome
- `t4207-log-decoration-colors` behavior is complete and validated against upstream tests (`4/4`).
- `PLAN.md` updated to `[x]` with note documenting the local mirror decode limitation.
