# t1512-rev-parse-disambiguation

- Fixed `rev-parse` handling of ambiguous short OIDs: parse `InvalidRef` before early return so full hint output matches Git.
- `grit-lib` `rev_parse`: pack-aware abbreviation matching; treeish/blob/commit context disambiguation; `resolve_revision_for_range_end`, `resolve_revision_as_commit`, describe-name parsing using `rev-list`-style generation count; peel-collapse for semi-ambiguous cases; `core.disambiguate` via `ConfigSet` (honours `-c`); branch ref wins over ambiguous hex with warning; `ambiguous_object_hint_lines` for sorted type-filtered hints; exported `parse_peel_suffix`.
- `commit-tree`: tree arg uses implicit tree-ish abbrev; parents use range-style commit-preferring resolution.
- `apply --build-fake-ancestor`: old blob index lines resolve with implicit blob abbrev.
- `log` / `reset`: use `resolve_revision_as_commit` for ranges and single revs.
- `rev_list` `split_revision_token`: use `split_double_dot_range` so `...` is not split as `..`.
- `rev-parse`: `--disambiguate`, verify fallback for describe strings, range ends use `resolve_revision_for_range_end`.
- `cat-file` batch: emit `ambiguous` + hints on ambiguous OID.
- `tests/lib-loose.sh`: set SHA1 prereq when default hash is sha1 so t1512 runs past early skip.
- `tests/t1512-rev-parse-disambiguation.sh`: `test_expect_failure` → `test_expect_success` for semi-ambiguous and describe-generation cases now implemented.

Validation: `./scripts/run-tests.sh t1512-rev-parse-disambiguation.sh` → 38/38; `cargo test -p grit-lib --lib`.
