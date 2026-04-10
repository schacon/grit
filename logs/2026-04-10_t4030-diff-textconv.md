# t4030-diff-textconv

## Goal

Make `tests/t4030-diff-textconv.sh` pass (19/19).

## Fixes

1. **`git show` commit diff** — Use the same textconv/binary gate as `git diff` (`diff_textconv_active` + `blob_text_for_diff_with_oid`) instead of bailing on NUL before textconv.
2. **Type change (blob → symlink)** — Expand `TypeChanged` into delete + add entries for porcelain `show` output so the deleted regular file still runs textconv; symlinks stay raw (matches upstream Git).
3. **`git log -p`** — Same textconv/binary handling as diff; emit `Binary files … differ` when appropriate.
4. **`git format-patch --no-binary`** — New flag; patch body skips textconv and emits binary placeholders (plumbing-style).
5. **Pickaxe textconv driver check** — Parse first token of `diff.*.textconv` like shell concatenation (`"/path"/script`) so validation matches runnable commands.
6. **`git show rev:path`** — Treat `rev:path` as a single revision token in argv splitting; raw blob by default; **`--textconv`** runs textconv for blob output only (matches Git).

## Validation

- `./scripts/run-tests.sh t4030-diff-textconv.sh` → 19/19
- `cargo fmt`, `cargo check`, `cargo clippy -p grit-rs --fix --allow-dirty`, `cargo test -p grit-lib --lib`
