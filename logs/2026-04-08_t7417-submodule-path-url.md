# t7417-submodule-path-url

## Goal

Make `tests/t7417-submodule-path-url.sh` fully pass (submodule path `-sub` in `.gitmodules`).

## Changes

1. **`grit-lib`**
   - New `gitmodules.rs`: `looks_like_command_line_option`, tree `.gitmodules` detection (HFS/NTFS), `write_gitmodules_cli_option_warnings`, `validate_gitmodules_blob_line`, `verify_gitmodules_for_commit`, `oids_from_copied_object_paths`.
   - `ConfigSet::load_repo_local_only` — read only `git_dir/config` + includes (no global/env), for receive-side fsck policy.
   - `escape_value`: quote values starting with `-` so `.gitmodules` round-trips.

2. **`grit mv`**
   - On gitlink rename: update matching `submodule.*.path` in `.gitmodules`, write blob to ODB, refresh index entry (commit must carry new blob).

3. **`grit push`**
   - After copying objects, if remote has `receive.fsckObjects` or `transfer.fsckObjects` (local config only), verify pushed commits’ `.gitmodules` blobs; on failure remove copied objects and emit Git-style `remote:` errors + `remote unpack failed`.

4. **`grit clone`**
   - Honor `GIT_QUIET=-q` from test harness (quiet clone).
   - Submodule recursion: warnings from `write_gitmodules_cli_option_warnings`; parse quoted `path`/`url`; resolve `./`/`../` URLs against `remote.origin.url` repository root (not clone work tree).
   - `clone_submodules` uses `repo` for config path.

## Validation

- `./scripts/run-tests.sh t7417-submodule-path-url.sh` → 5/5
- `cargo test -p grit-lib --lib`

## Note

AGENTS.md says not to use GitButler; user rule mentioned MCP update — skipped in favor of project rules.
