# t6017-rev-list-stdin

## Goal

Make `tests/t6017-rev-list-stdin.sh` fully pass (37/37).

## Changes (summary)

- **grit-lib `rev_list`**: Rewrote `collect_revision_specs_with_stdin` to mirror Git `read_revisions_from_stdin`: line-based parsing, stdin-only `--not` toggle, `handle_revision_pseudo_opt`-style options (`--glob`, `--all` under `--not` includes HEAD), `--end-of-options`, pathspec tail after stdin `--`, Git-compatible fatal messages via `Error::Message`. Added `resolve_revision_specs_to_commits` for callers.
- **grit-lib `refs`**: Fixed `list_refs_glob` for patterns without wildcards (prefix `refs/heads` + `ref_matches_glob`).
- **grit `rev-list`**: Pseudo-options (`--glob`, `--branches`, …) and `--all` under `--not` now emit `^<oid>` specs; stdin pathspecs appended to options.
- **grit `log`**: `--stdin` flag; preserve raw argv tail in `main` (strip `--not`/`--glob` for clap only); `merge_log_revision_argv` + unified walk setup; `allow_hyphen_values` on revisions; pathspecs from merged argv after `--`; filter `--name-only`/`--name-status`/`--raw` entries by effective pathspecs; blank line before name list when needed.
- **Harness**: `run-tests.sh t6017-rev-list-stdin.sh` → CSV + dashboards.

## Validation

- `cargo fmt`, `cargo clippy -p grit-rs -p grit-lib --fix --allow-dirty`
- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t6017-rev-list-stdin.sh` → 37/37
