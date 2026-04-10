# t7411-submodule-config

- Implemented `test-tool submodule-config` and `test-tool submodule-nested-repo-config` in `grit/src/main.rs`.
- Added `grit_lib::submodule_config_cache` for `.gitmodules` blob caching, path/name lookup, and nested submodule config printing.
- Extended `ConfigFile` parsing: unclosed-quote continuation (Git-style) and `parse_gitmodules_best_effort` so valid lines before a bad line still apply (matches Git submodule-config streaming).
- `HEAD` in submodule-config resolves via `resolve_head` so detached checkout does not change the blob label OID in stderr.
- Harness: `./scripts/run-tests.sh t7411-submodule-config.sh` → 20/20.
