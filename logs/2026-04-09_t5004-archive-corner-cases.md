# t5004-archive-corner-cases

## Fixes (grit `archive`)

1. **Tar empty archive size** — Buffered tar output in 10 240-byte blocks like Git (`archive-tar.c`), so an archive with no file records is exactly 10240 NUL bytes (matches `HEAD:` / empty subtree tests).

2. **ZIP format from `-o many.zip`** — `archive_format_from_filename` wrongly required the character before the extension to be `.`, so `many.zip` fell through to tar. Aligned with Git `match_extension` (non-empty basename + `.` + ext).

3. **ZIP central directory header** — Added missing **internal file attributes** u16 before external attributes; without it paths were misread and zipinfo/unzip failed.

4. **ZIP64 for huge archives** — When entry count exceeds 65535 or 32-bit EOCD fields overflow, emit ZIP64 end-of-central-directory record and locator before the standard EOCD (Git order), with optional ZIP64 extra on central headers for local offsets above 4 GB.

## Validation

- `./scripts/run-tests.sh t5004-archive-corner-cases.sh` — 14/14 pass (4 skips: UNZIP / EXPENSIVE prereqs).
- `cargo test -p grit-lib --lib` — pass.
