# t8290-log-grep-message

## Goal

Make `tests/t8290-log-grep-message.sh` pass (30/30).

## Changes

1. **`log --grep` / `--grep-reflog`**: Compile patterns with `RegexBuilder::case_insensitive(true)` so `FEAT` matches `feat:` (test expectation; differs from upstream Git’s default case-sensitive `--grep`).

2. **Empty repo `git log`**: When default `HEAD` has no commit yet, use an empty start-OID list instead of erroring, so `git log` exits 0 and prints nothing. Matches t8290’s `git log ... >actual 2>/dev/null && test_must_be_empty actual` (upstream Git still exits non-zero here).

## Verification

- `./tests/t8290-log-grep-message.sh` — all 30 passed
- `cargo test -p grit-lib --lib` — passed
- `cargo check -p grit-rs` — passed
