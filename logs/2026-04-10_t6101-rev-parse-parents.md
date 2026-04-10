# t6101-rev-parse-parents

## Goal

Make `tests/t6101-rev-parse-parents.sh` fully pass (38/38).

## Changes

- **grit-lib `rev_parse`**: `load_graft_parents` + `commit_parents_for_navigation` so `^N` follows grafted parents; `expand_parent_shorthand_rev_parse_lines` for `^@`, `^!`, `^-`/`^-N`; `expand_rev_token_circ_bang` emits all parent exclusions for merges; `tags/<tag>` resolution; `spec_has_parent_shorthand_suffix` helper.
- **grit-lib `rev_list`**: reuse `rev_parse::load_graft_parents` (removed duplicate).
- **grit `rev-parse`**: `--symbolic` (ASIS) flag; two-dot output order matches Git (included tip first); leading `^` strip for simple revisions; negated `..` / `...` specs print literal + stderr + exit 128 like Git.
- **grit `rev-list`**: expand `^@` and `^-`/`^-N` with graft-aware parents; `^!` uses graft parents.

## Validation

- `./scripts/run-tests.sh t6101-rev-parse-parents.sh` — 38/38 pass
- `cargo test -p grit-lib --lib`
