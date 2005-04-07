# t5618-alternate-refs

- Fixed `list_refs` prefix handling (directory vs single ref, packed-refs exact match) and `list_refs_glob` exact patterns via `ref_matches_glob`.
- Added `refs::collect_alternate_ref_oids` with `core.alternateRefsPrefixes`; wired `rev-list --alternate-refs` and `--not` pairing; `receive-pack` default alternate OIDs use `list_refs`.
- `log`: `--remotes[=pat]` (preprocessed), `--alternate-refs`, remote-tracking / alternate `--source` maps; oneline `--source` order matches Git (`hash\\tsource subject`).
- Test harness: `cd child` per case (Gust resets cwd); clear `alternateRefsPrefixes` before test 6.
