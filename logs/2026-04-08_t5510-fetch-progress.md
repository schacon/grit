# t5510-fetch progress (2026-04-08)

## Changes

- **Clone `--ref-format`**: `grit clone` accepts `--ref-format=files|reftable`; init path writes `extensions.refStorage` and empty reftable stack when needed.
- **Reftable clone**: ref copy / checkout paths use `refs::` + symref writes instead of only loose files under `refs/`.
- **Fetch default remote**: bare `git fetch` uses `branch.<HEAD>.remote` when set (matches Git).
- **FETCH_HEAD merge flags**: align with Git `get_ref_map` / `add_merge_config` (merge when `branch.*.merge` matches remote ref; else first ref only for default `+refs/heads/*` refspec).
- **`remote.<name>.followRemoteHEAD`**: symbolic `refs/remotes/<name>/HEAD`, create/warn/always/never + warnings; trace line for `ref-prefix HEAD` when applicable.
- **`git remote set-head <remote> <branch>`**: sets symref for tests.
- **tests/lib-bundle.sh**: removed accidental merge conflict markers.

## Harness

- `./scripts/run-tests.sh t5510-fetch.sh`: **93 pass / 122 fail / 10 skip** (215 total) at time of commit — improved from 81 pass.

## Notes

Full suite still needs bundle, `--atomic`, `--refmap`, HTTP negotiation, etc.
