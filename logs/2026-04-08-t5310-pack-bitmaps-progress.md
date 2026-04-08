# t5310-pack-bitmaps progress

## Changes

- `grit-lib`: added `pack_name_hash` / `pack_name_hash_v2` matching Git `pack-objects.h` + unit test for t5310 vectors.
- `grit fast-import`: support `commit` stream used by `test_commit_bulk` (author/committer, heredoc message, optional `from`, `M 644 inline` with nested paths); chain commits per ref when `from` is omitted (matches git fast-import).
- `grit test-tool name-hash`: stdin lines → v1/v2 hashes (t5310 first test).
- `tests/test-lib-commit-bulk.sh`: upstream `test_commit_bulk` + `BUG`; `tests/lib-bitmap.sh` sources it so bitmap tests get real bulk history.

## Test result

`./scripts/run-tests.sh --timeout 600 t5310-pack-bitmaps.sh`: **127/236** pass (was ~22).

Remaining failures need real pack bitmaps: `.bitmap` generation on repack, `rev-list --test-bitmap`, bitmap-backed `--use-bitmap-index` object ordering, fetch/pack-objects bitmap paths, trace2 events, etc.

## Validation

- `cargo test -p grit-lib --lib` — pass (includes `pack_name_hash` test).
- `cargo clippy -p grit-rs -- -D warnings` — fails due to pre-existing grit-lib clippy denies (not introduced here).
