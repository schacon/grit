# t5606-clone-options

## Summary

Made `tests/t5606-clone-options.sh` pass (21/21).

## Changes

- **clone** (`grit/src/commands/clone.rs`): `-o`/`--origin`, `--reject-shallow` / `--no-reject-shallow`, `clone.rejectshallow` config, `--bundle-uri` vs shallow conflict, shallow warnings for local clones, `--progress` lines for `file://`, `clone.defaultRemoteName` and `-c clone.defaultRemoteName=`, sticky `submodule.recurse` when `submodule.stickyRecursiveClone` + `--recurse-submodules`, remote name validation, HEAD/checkout guessing (unborn default branch, detached HEAD), no `refs/remotes/<remote>/HEAD` when source HEAD is dangling, bare+separate-git-dir error message order.
- **config** (`grit/src/commands/config.rs`): global `-C <path>` before discovery.
- **symbolic-ref** (`grit/src/commands/symbolic_ref.rs`): read symref target when ref file missing (dangling).
- **init from template** (`grit-lib/src/repo.rs`): merge template `config` over default; skip template `core.bare`.
- **test-lib** (`tests/test-lib.sh`): `test_config` / `test_unconfig` support `-C` and `--worktree` like upstream (fixes `test_config -C empty ...`).

## Validation

- `./scripts/run-tests.sh t5606-clone-options.sh` → 21/21 pass
- `cargo test -p grit-lib --lib` → pass
