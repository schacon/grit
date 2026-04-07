# t13070 for-each-ref --points-at

## Issue

`t13070-for-each-ref-points-at.sh` failed on "for-each-ref --points-at nonexistent SHA errors": `resolve_revision` returns a parsed 40-hex OID even when the object is absent (Git `rev-parse` behavior), so `--points-at` produced empty output with exit 0 instead of failing.

## Fix

In `apply_filters`, after resolving `--points-at`, verify the object exists in the ODB via `repo.odb.read` before filtering refs.

## Validation

- `./scripts/run-tests.sh t13070-for-each-ref-points-at.sh` — 32/32 pass
- `cargo test -p grit-lib --lib` — pass
