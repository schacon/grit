# t7601-merge-pull-config

## Summary

Made `tests/t7601-merge-pull-config.sh` pass (65/65).

## Changes

- **pull**: Multiple refspecs (`git pull . c1 c2`), `FETCH_HEAD` layout, `pull.rebase` / `pull.ff` precedence matching `git/builtin/pull.c` (divergent-branch advice, ff-only vs rebase, multi-head errors). Normalized `--rebase . c1` argv when clap consumes `.` as the optional `--rebase` value.
- **merge**: `pull.twohead` / `pull.octopus` from config; reject octopus with only `recursive`/`ort`; disable rename detection for `resolve` strategy; multi-strategy `-s` tries with conflict-path scoring and final re-run of best strategy (t7601 auto merge).
- **add**: Removed stat-cache-only skip in `stage_file` so rapid `echo` + `add` updates the index (fixes test setup where `c6` omitted `conflict.c`).

## Validation

- `./scripts/run-tests.sh t7601-merge-pull-config.sh` — 65/65
- `cargo test -p grit-lib --lib`
