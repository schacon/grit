# format-patch / t4014 progress

## Done this session

- Fixed `rev_list` default ordering when `--max-parents=1` filters out merge tips: seed date-order walk from all parents of filtered tips (matches `git rev-list` on merge HEAD).
- Rewired `format-patch` commit selection through `rev_list` with `max_parents=1`, `reverse`, optional `max_count`; `main..side` and `format-patch <since>` → `<since>..HEAD` behavior.
- Implemented `--ignore-if-in-upstream` using patch-id collection from the upstream side of a two-dot range.
- Added `--no-from`, `--no-to`, `--no-cc`, `--no-add-header`; `format.from` and folded Cc merging for config + `--add-header`.
- `format.noprefix` strict boolean with Git-style fatal + hints; diff `--git` line and unified hunks honor noprefix / `--default-prefix`.
- `format.filenameMaxLength` from config; preprocess `-N` count for clap via `--grit-format-patch-max-count` in `main.rs`.

## Test status

- `./scripts/run-tests.sh t4014-format-patch.sh`: **39 / 215** pass (many tests still need threading, reroll `-v`, cover letter, notes, MIME attach, range-diff/interdiff, etc.).

## Reason not complete

- **blocked** on remaining Git parity surface area in `format-patch` (large test file).
