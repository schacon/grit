# 2026-04-09 — mergetool vimdiff layout in grit-lib

- Added `grit-lib::mergetool_vimdiff` porting Git `mergetools/vimdiff` `gen_cmd` / layout resolution / no-base buffer renumbering.
- Unit tests mirror `tests/t7609-mergetool--lib.sh` expectations (19 layout cases + argv shape without base).
- `grit mergetool` uses this for `vimdiff` / `gvimdiff` / `nvimdiff` (and numbered variants): `vim -f -c '...'`, `mergetool.<tool>.path`, layout keys, copy LOCAL/REMOTE to MERGED on success when `@`-target says so.
