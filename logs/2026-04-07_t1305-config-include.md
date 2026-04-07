# t1305-config-include

## Goal

Make `tests/t1305-config-include.sh` pass (37/37).

## Changes

- **grit-lib `config`**: Git-compatible `[include]` / `[includeIf]` with `gitdir:`, `gitdir/i:`, `onbranch:`; `LoadConfigOptions` / `IncludeContext`; correct wildmatch polarity; `GIT_CONFIG_PARAMETERS` merged via structured entries + include expansion; `ConfigIncludeOrigin` to distinguish disk vs stdin/blob/command-line for relative include rules; `read_early_config` for `test-tool config read_early_config`.
- **grit `config`**: Lookup vs list include expansion (scoped `--global` single-key vs `--list`); stdin includes; blob includes; `-c` ordering by prepending to `GIT_CONFIG_PARAMETERS`.
- **grit `main`**: Merge multiple `-c` with existing `GIT_CONFIG_PARAMETERS`; prepend order so first `-c` wins.
- **Discovery**: Prefer `PWD` over cwd when canonical paths match (symlink `gitdir:` tests).
- **scripts/run-tests.sh**: `GIT_CONFIG_NOSYSTEM=1` and clear `GIT_CONFIG_PARAMETERS` for isolated runs.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t1305-config-include.sh` → 37/37
