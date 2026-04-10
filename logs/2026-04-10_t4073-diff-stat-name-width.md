# t4073-diff-stat-name-width

- **Issue:** `--stat` name column used Rust `{:<n$}` padding (Unicode scalar count). Git pads to `name_width` in **display columns** (`utf8_strnwidth`), so wide characters (e.g. CJK) needed different trailing space counts.
- **Fix:** `grit-lib/src/diffstat.rs`: `pad_name_to_display_width()` appends ASCII spaces until `unicode_width` display width reaches `name_width`; use that for both text and binary stat lines instead of format padding.
- **Tests:** `cargo test -p grit-lib --lib`; `./scripts/run-tests.sh t4073-diff-stat-name-width.sh` → 6/6.
- **Note:** `./scripts/run-tests.sh` copies `target/release/grit`; builds must update that path (default workspace `cargo build --release`) or the harness can run a stale binary.
