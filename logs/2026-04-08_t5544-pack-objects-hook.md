# t5544-pack-objects-hook

## Goal

Make `tests/t5544-pack-objects-hook.sh` pass (7/7).

## Root causes

1. `git clone --no-local` did not run `upload-pack`; it used direct object copy, so `pack-objects` (and any hook) never ran.
2. Protocol v2 `serve-v2` `fetch` returned an empty response while clients negotiated v2 by default, which could break clones once v0 packing worked.
3. `upload-pack` always spawned `grit pack-objects` with a bare want list; Git passes `--revs`, `--thin`, `--not`, and haves on stdin.
4. `uploadpack.packObjectsHook` must come from **protected** config (system + global + command-line), not repo `config`.

## Changes

- `grit-lib::config::ConfigSet::load_protected()` — mirrors Git `read_protected_config` (no repo/worktree, no `$GIT_CONFIG`).
- `grit upload-pack` — read hook from protected set; spawn `sh -c 'exec "$0" "$@"' <hook> git pack-objects --revs --thin --stdout --progress --delta-base-offset`; stdin: wants, `--not`, have OIDs, blank line; track `have` commits for stdin.
- `grit pack-objects` — `--revs` parses `--not` / post-not haves; `--thin` drops reachable objects from haves; accept no-op flags matching Git’s argv (`--shallow-file`, `--include-tag`, …).
- `grit clone` — when `--no-local` and not `--shared`, populate objects via `fetch_via_upload_pack_skipping` instead of copying; removed broken early path that ran custom `-u` as a one-shot shell without building a repo.
- `serve_v2::cmd_fetch` — protocol v2 `fetch` sends `acknowledgments` / `ready` when appropriate, then `packfile` + raw pack bytes (shared `pack_objects_upload` helper with v0).
- `pack_objects_upload` — shared spawn + stdin + drain (v0 uses side-band-64k wrapper).

## Verification

- `GUST_BIN=target/release/grit bash tests/t5544-pack-objects-hook.sh` — 7/7
- `./scripts/run-tests.sh t5544-pack-objects-hook.sh` — 7/7
