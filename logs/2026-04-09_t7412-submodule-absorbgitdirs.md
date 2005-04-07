# t7412-submodule-absorbgitdirs

## Goal

Make `tests/t7412-submodule-absorbgitdirs.sh` pass 12/12 with grit as `git`.

## Changes (summary)

- **`submodule absorbgitdirs`**: Walk index gitlinks (with `.gitmodules` name lookup), match Git’s stderr migration format, recurse into submodules via `submodule--helper absorbgitdirs --super-prefix=…`, handle already-under-common-dir gitdirs, gitfile repair, and multi-worktree rejection.
- **`submodule--helper`**: Dispatch `absorbgitdirs` with `--super-prefix` / `-q`.
- **`fsck`**: Do not traverse into gitlink OIDs (submodule commits live in separate object stores); fixes false “missing blob” after submodule workflows.
- **`submodule update`**: Resolve `.git/modules/<name>/` using submodule **name** (not path); `run_update` enables implicit nested recursion like Git; skip checkout + progress when HEAD already matches recorded gitlink (quiet `submodule update`); `default_remote_url_raw` reads config from **common** git dir (linked worktrees).
- **`checkout_submodule_worktree`**: Takes submodule name for modules path.

## Validation

- `cargo fmt`, `cargo clippy -p grit-rs -p grit-lib --fix --allow-dirty`
- `cargo test -p grit-lib --lib` — 160 passed
- `./scripts/run-tests.sh t7412-submodule-absorbgitdirs.sh` — 12/12
