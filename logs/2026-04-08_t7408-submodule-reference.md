# t7408-submodule-reference

## Goal

Make `tests/t7408-submodule-reference.sh` pass: `clone` / `submodule` behaviour for `--reference`, `--reference-if-able`, `--dissociate`, and recursive superproject clones with `submodule.alternateLocation=superproject`.

## Changes (Grit)

- **`grit clone`**: `--reference-if-able`, `--dissociate`; merge source `objects/info/alternates` into the destination with absolute paths; append required/optional reference object dirs; omit the extra “local clone → source objects” alternate line when any `--reference*` is used (single-line `alternates` like Git); on `--recursive`, write `submodule.alternateLocation` / `submodule.alternateErrorStrategy`; remove the destination tree if recursive submodule clone fails after partial success; `--dissociate` runs `repack -a -d` and unlinks `objects/info/alternates`.
- **Recursive submodule clone**: parse `.gitmodules` with submodule **names**; clone with `--separate-git-dir` under `.git/modules/<path>`; derive `--reference` from superproject alternates (`…/modules/<path>`); retry clone without derived refs if the first attempt fails; set `core.worktree` via `set_submodule_core_worktree_after_separate_clone`.
- **`grit submodule`**: `update --reference` / `--dissociate`, `add --reference` / `--dissociate`; superproject-derived references + retry without them on failure.

## Validation

Run locally after `cargo build --release -p grit-rs` and `cp target/release/grit tests/grit`:

```bash
./scripts/run-tests.sh t7408-submodule-reference.sh
```

Expect 16/16 pass and CSV row updated to `16	16`.
