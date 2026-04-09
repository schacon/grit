# t3902-quoted — path quoting

## Problem

`git ls-files`, `git diff --name-only`, and `git ls-tree --name-only` did not match Git’s C-style quoting (`quote.c`) or `core.quotepath` semantics. Paths with `\n`, `\t`, `"`, or non-ASCII bytes were emitted raw or with wrong escaping.

## Fix

- Added `grit_lib::quote_path::quote_c_style` mirroring Git’s `cq_lookup` + `quote_path_fully` behavior.
- `ConfigSet::quote_path_fully()` reads `core.quotepath` / `core.quotePath` (default true).
- Wired quoting through `ls-files`, `ls-tree --name-only`, and `diff` name-only/name-status/stat/summary paths; `numstat` keeps unquoted raw paths.

## Verification

- `./scripts/run-tests.sh t3902-quoted.sh` — 13/13.
- `cargo test -p grit-lib --lib` — pass.
