# t7506-status-submodule

## Summary

Made `tests/t7506-status-submodule.sh` pass (40/40).

## Changes (high level)

- **Submodule dirty detection** (`grit-lib/diff.rs`): gitlink index↔worktree diff matches Git (inner dirty + `-uno` untracking); nested gitlink scan uses child submodule index for untracked detection; `diff_index_to_worktree` gained `simplify_gitlinks` for nested porcelain flags.
- **status** (`grit/src/commands/status.rs`): long-format submodule suffix lines; short/porcelain XY for submodules; unmerged short uses Git conflict letters (`AA`, etc.); porcelain v1 `##` heuristic; `--porcelain=2` → v2; porcelain v2 submodule tokens with `-uno`.
- **commit --dry-run** (`commit.rs`): footer “nothing to commit…”; `find_untracked_files` respects ignore rules; `auto_stage_tracked` no-op when gitlink OID unchanged; submodule add `-f`/`--force`.
- **merge** (`merge.rs`): allow merge when untracked path is leftover populated submodule dir.
- **diff** (`diff.rs`): combined conflict patch when stage 1 missing (add/add).

## Validation

- `./scripts/run-tests.sh t7506-status-submodule.sh` → 40/40
- `cargo test -p grit-lib --lib`
- `bash tests/t12570-status-rename-copy.sh`
