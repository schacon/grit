## t4207-log-decoration-colors

- Implemented Git-compatible `--decorate` coloring for `grit log --oneline --color=always`: `diff.color.commit` for hash/punctuation, `color.decorate.*` slots, HEAD → branch merge, tag prefix coloring, remotes, stash, replace/graft `replaced` marker; `GIT_REPLACE_REF_BASE` respected when scanning replace refs.
- Log walk now uses `Repository::read_replaced` so replace objects affect parent chain (matches `git log` for replace/graft tests).
- `--no-abbrev` now uses full 40-char hashes in oneline output.
- `tests/test-lib.sh`: replaced sed-based `test_decode_color` with awk decoder (matches upstream) so combined SGR like `\x1b[1;7;33m` decodes to `<BOLD;REVERSE;YELLOW>` as tests expect.
- `format_ansi_color_spec`: emit SGR parameter order aligned with Git (bold before reverse) for combined sequences.

Validation: `./scripts/run-tests.sh t4207-log-decoration-colors.sh` → 4/4; `cargo test -p grit-lib --lib`; spot-check `t3205-branch-color.sh`.
