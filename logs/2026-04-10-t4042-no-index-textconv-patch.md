## t4042 — `diff --no-index` patch body vs textconv

- **Failure:** Test 8 (`caching is silently ignored outside repo`) expected hunks with textconv output (`ONE` / `TWO`) while grit showed raw file lines (`one` / `two`).
- **Cause:** `run_no_index` applied textconv for equality / stat / exit code but called `no_index_unified_patch_body` with raw `data_a` / `data_b`.
- **Fix:** Pass `text_a.as_bytes()` / `text_b.as_bytes()` into `no_index_unified_patch_body` so the unified diff matches the converted view (index lines still use blob hashes of raw files, matching Git).

Verification:

- `bash tests/t4042-diff-textconv-caching.sh` → 8/8
- `./scripts/run-tests.sh t4042-diff-textconv-caching.sh` → 8/8
- `cargo test -p grit-lib --lib` → pass
