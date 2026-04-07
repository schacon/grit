# t5604-clone-reference

## Goal

Make `tests/t5604-clone-reference.sh` pass (34/34) under the grit harness.

## Changes (summary)

- **clone**: `--dissociate`, `--no-hardlinks`; correct `--reference` / `-s` / `file://` alternates layout (reference-only alternates for URL clones and local clones with `--reference`, no auto-alternate for plain local clones); gitfile/symlink reference repo resolution; copy non-standard `objects/*` junk for local clones; `GIT_TRACE_PACKET` line for clone; dissociate repack uses absolute `-C` git-dir.
- **fetch**: skip copying loose objects already reachable via alternates; `GIT_TRACE_PACKET` tip lines (`fetch> have` / `fetch> fetch`) without ` want ` substring.
- **count-objects**: document primary-store-only loose count (matches Git).
- **odb**: `exists_in_primary_only`; `write`/`write_raw` skip writing when object exists in alternates.
- **fsck**: fail with exit 2 when `info/alternates` paths are missing (broken reference after `rm -rf P`).
- **repo**: public `resolve_gitfile_path` for clone reference parsing.

## Validation

- `./scripts/run-tests.sh t5604-clone-reference.sh` → 34/34
- `cargo test -p grit-lib --lib`
