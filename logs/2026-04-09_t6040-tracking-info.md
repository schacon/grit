# t6040-tracking-info

## Summary

Made `tests/t6040-tracking-info.sh` pass (44/44).

## Changes

- **rev_parse**: Resolve `origin`/`upstream` via `refs/remotes/<name>/HEAD`; branch vs tag ambiguity prefers branch with warning; exported push resolution (`resolve_push_full_ref_for_branch`) with `push.default`, `remote.pushDefault`, and `remote.<name>.push` refspec mapping.
- **merge_base**: `count_symmetric_ahead_behind` for Git-style `A...B` counts (used by branch `-v` and tracking).
- **checkout**: Default `branch.autoSetupMerge` when unset; remote-only start (`origin`) vs `remote/branch` (`origin/main`) tracking setup; avoid bogus `refs/remotes/origin/main/HEAD`; print `format_tracking_info` on "Already on" branch path; `open_repo` in push resolves `.git` gitfile for path remotes.
- **push**: `open_repo` gitfile fix so `git push` from clones works.
- **status**: Hidden `--long`; `status.aheadbehind` + `--ahead-behind`; symmetric upstream stats; `status.compareBranches` from `[status]` section; porcelain short branch line uses quick/full mode.
- **commit**: `--dry-run` before "nothing to commit" bail; exit 1 on clean dry-run; `--no-ahead-behind` / config; tracking lines via shared helper.
- **branch**: Symmetric ahead/behind; `-vv` in-sync shows `[origin/main]`; reject `--track` to tag-only start; `branch --set-upstream-to @{-N}`.
- **branch_tracking.rs**: Shared `format_tracking_info` / `stat_branch_pair` matching Git `remote.c` behavior.

## Validation

- `./scripts/run-tests.sh t6040-tracking-info.sh` → 44/44
- `cargo test -p grit-lib --lib`
