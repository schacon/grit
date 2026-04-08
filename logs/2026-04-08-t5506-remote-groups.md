# t5506-remote-groups

## Changes

- `git remote add -m <branch>`: write `refs/remotes/<name>/HEAD` symbolic ref (Git-compatible).
- `remote update` / group expansion: read all `remotes.<group>` values via `ConfigSet::get_all` (supports `git config --add`).
- `git fetch <group>`: same multi-value group resolution.
- Revision DWIM: resolve a bare remote name to the OID of `refs/remotes/<name>/HEAD` when `remote.<name>.url` exists (fixes `git log -1 one` in the test).

## Verification

- `./scripts/run-tests.sh t5506-remote-groups.sh` → 9/9 pass.
