# t4013 / diff-tree + combined diff (2026-04-09)

## Done

- **Merge commits, single-arg `diff-tree`**: Skip diff unless `-m`, `-c`/`--cc`, or remerge-diff (matches Git).
- **`-m`**: Repeat OID per parent; `--pretty` adds `(from <parent>)` on subject/oneline/medium headers.
- **Stdin mode**: Merge + `-s` prints nothing; merge without `-m`/combined prints only commit id; `-m` repeats id per parent; combined stdin uses same stat/raw/patch ordering as non-stdin.
- **`--stdin` pathspecs**: Positional args after `--stdin` are pathspecs only (fix `diff-tree --stdin … dir`).
- **`--format=%s`**: Parsed for stdin; merge without combined prints nothing; subject + blank line before diff when combined.
- **Combined diff**: Recursive multitree walk default; intersect paths with `combined_diff_paths`; raw lines `::…`; first-parent stat for `-c/--cc --stat`; **`--cc` alone defaults to patch**, `-c` alone stays raw; patch hunks fixed via `similar` alignment to match Git `diff --cc`.
- **Two-tree `--summary` only**: Omit raw `:` lines unless `--raw` explicitly given; mode-only summary requires same blob OID on both sides.
- **`whatchanged`**: Fail with upstream-style message unless `--i-still-use-this`.

## Still failing in t4013 (122 tests)

Mostly `log`, `show`, `format-patch`, `diff` (dirstat, prefixes, `-I`, line-prefix), `diff-tree` notes/compact-summary, `rev-list --children`, decorate tests.

## Commands

```bash
cargo build --release -p grit-rs
./scripts/run-tests.sh t4013-diff-various.sh
```
