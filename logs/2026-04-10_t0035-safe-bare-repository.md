# t0035-safe-bare-repository

## Symptom

Harness reported 9/12 passing. Manual run showed:

1. Setup failed: `worktree add` before first commit errored with `invalid reference: HEAD` (Git infers `--orphan` when repo has no local refs).
2. After fixing worktree, setup still failed: `submodule add --name subn` created `.git/modules/subd` instead of `.git/modules/subn`.

## Fixes

- `grit/src/commands/worktree.rs`: implement Git `can_use_local_refs` / `dwim_orphan` / `can_use_remote_refs`; merge explicit `--orphan` with inferred orphan; path-only branch resolution matches `dwim_branch` order.
- `grit/src/commands/submodule.rs`: `clone --separate-git-dir` target uses submodule **name** (`--name` or path) for `submodule_modules_git_dir`.

## Validation

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t0035-safe-bare-repository.sh` → 12/12
