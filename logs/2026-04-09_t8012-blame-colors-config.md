# t8012 blame colors — config parity

## Work

- Wired `blame.coloring` (`repeatedLines`, `highlightRecent`, `none`) to enable `--color-lines` / `--color-by-age` when flags are omitted, matching Git.
- Parsed `color.blame.repeatedLines` via `parse_color`; default repeated-line color is cyan when unset (Git default).
- Parsed `color.blame.highlightRecent` with Git’s comma state machine and `approxidate_careful`; default spec matches Git (`blue,12 month ago,white,1 month ago,red`).
- Replaced wall-clock age heuristic in `write_default` with `determine_line_heat`-style bucket walk.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t8012-blame-colors.sh` (120/120)
