# t6021 rev-list --exclude-hidden

## Goal

Make `tests/t6021-rev-list-exclude-hidden.sh` pass (Git parity for `rev-list --exclude-hidden`).

## Changes

- Added `grit-lib::ref_exclusions`: `transfer.hideRefs` + `<section>.hideRefs` parsing (canonical `hiderefs` keys), Git-style `ref_is_hidden` prefix rules (`!`, `^`), `GIT_NAMESPACE` → `refs/namespaces/.../` prefix for stripped matching.
- `RevListOptions`: `ref_exclusions`, `all_refs_passes: Vec<(bool, RefExclusions)>` so each `--all` records current `--not` mode and exclusion snapshot (matches Git argument order).
- `rev_list`: merge tips from passes into include vs exclude; allow empty result when `--all` yields no tips after hiding; `commit_tips_from_named_refs` for `--glob` / pseudo-refs.
- `grit rev-list`: `--exclude`, `--exclude-hidden`, conflict errors with pseudo-refs (exit 129), fatal messages via `LibError::Message` for verbatim stderr.

## Validation

- `./scripts/run-tests.sh t6021-rev-list-exclude-hidden.sh` — 62/62
- `cargo test -p grit-lib --lib`
