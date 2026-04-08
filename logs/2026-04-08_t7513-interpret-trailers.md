# t7513-interpret-trailers

- Added `grit_lib::interpret_trailers` porting Git `trailer.c` behaviour: trailer block detection (--- divider, cut line, ignored tail), `trailer.*` config (two-pass global defaults + per-alias), `process_trailers_lists`, command/`cmd` execution via `sh` with stdin closed, `core.commentChar` for comment lines.
- Replaced clap-based `interpret-trailers` with manual argv parsing so `--where` / `--if-exists` / `--if-missing` apply per following `--trailer` like Git.
- Harness: `./scripts/run-tests.sh t7513-interpret-trailers.sh` → 99/99 pass.
