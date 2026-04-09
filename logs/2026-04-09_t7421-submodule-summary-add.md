# t7421-submodule-summary-add

## Outcome

All 5 tests in `tests/t7421-submodule-summary-add.sh` pass.

## Changes

1. **`git submodule summary`** — Implemented Git-style summary: diff index vs base commit tree for gitlinks, pathspec filtering, deinit skip (no `.git` in work tree), relocated submodule path via `.gitmodules`, `rev-list`/`log` in submodule, flush before child log to avoid stdout interleaving, first-line-only `rev-parse --short` (trailing `--`).

2. **`submodule update --remote`** — Local `remote.origin.url` fast path: copy reachable objects from source repo into submodule git dir and update `refs/remotes/origin/*` without upload-pack (fixes stale remote-tracking refs when protocol v2 advertises no refs to the v0 client). Falls back to `grit fetch origin` for non-local URLs.

3. **Index** — After remote checkout, stage updated gitlink in superproject index (`stage_gitlink_in_super_index`) so `git commit <path>` works.

4. **`SummaryArgs`** — Extended for `--cached`, `--files`, `-n` / `--summary-limit`, and trailing `[commit] [--] [paths…]`; top-level `--cached` merges into summary.

5. **`copy_reachable_objects`** — `pub(crate)` for reuse from submodule fetch.

6. **`fetch_via_upload_pack_skipping`** — When wants are empty but refs were advertised, still return heads/tags so `git fetch` can update remote-tracking refs (kept for non-submodule cases).

7. **`merge_git_protocol_env_for_child`** — Strip existing `version=` entries from `GIT_PROTOCOL` before appending client version (avoids `version=1:version=2` overriding).
