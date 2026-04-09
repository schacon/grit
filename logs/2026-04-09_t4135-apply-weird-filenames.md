# t4135-apply-weird-filenames

## Goal

Make harness file `t4135-apply-weird-filenames.sh` pass all tests that are expected to pass (19/20; one remains `test_expect_failure` for quoted traditional patch).

## Changes (grit `apply` patch parsing)

- Ported Git `apply.c` traditional header logic: `diff_timestamp_len`, `find_name_traditional`, `parse_traditional_patch` semantics (including `.orig` shortening via `def`, same `+++` path for both sides when no epoch marker).
- `has_epoch_timestamp` + fixed `tz_with_colon_len` (7-byte ` ±HH:MM` suffix) for `funny-tz.diff` / `damaged-tz.diff`.
- `git_header_name` / `split_diff_git_paths`: C-quoted `diff --git` paths, repeated unquoted names with spaces, end-of-line boundary when `lines()` strips `\n`.
- `---`/`+++` for git diffs: treat `/dev/null` literally (do not run `find_name` with `p=1`).
- Extended headers `rename from` / `copy from`: strip with `p_value - 1` like Git.

## Verification

- `./scripts/run-tests.sh t4135-apply-weird-filenames.sh` → 19/20 pass (expected).
- `cargo fmt`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`.
