# t7816-grep-binary-pattern

## Problem

`git grep -f` with patterns containing embedded NUL was wrong: `read_to_string` truncated at the first NUL, so matchers saw a shortened pattern and reported spurious "Binary file matches" instead of Git's fatal error (without `-P`) or correct PCRE-style behavior (with `-P`).

## Fix

- Read pattern files as raw bytes; split on `\n` only; preserve NUL in UTF-8 patterns.
- After loading patterns, if any contains NUL and `-P` is not active, bail with Git's message (exit 128 via `fatal:` prefix).
- For binary blob/file "matches" output, run regex against valid UTF-8 `&str` when possible so NUL positions align with Git (REG_STARTEND-style), instead of lossy replacement.

## Validation

- `./scripts/run-tests.sh t7816-grep-binary-pattern.sh` → 145/145 pass.
- `cargo check -p grit-rs`, `cargo test -p grit-lib --lib` pass.

## Misc

- Removed stray blank line between doc comment and `resolve_index_path` in `rev_parse.rs` (clippy `empty_line_after_doc_comment`).
