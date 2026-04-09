# t5402-post-merge-hook

## Goal

Make `tests/t5402-post-merge-hook.sh` pass (7/7).

## Changes

1. **`grit merge` — `post-merge` hook** (`grit/src/commands/merge.rs`)
   - Added `run_post_merge_hook` calling `run_hook` with `"0"` or `"1"` (squash), matching upstream `merge.c` `finish()`.
   - Invoked after successful: fast-forward, real merge commit, `--no-commit` clean auto-merge, octopus success, `-s ours`/`theirs`, and squash paths (`do_squash`, `do_squash_from_merge`, octopus squash).

2. **Fast-forward when index already matches merge tip** (`merge.rs`)
   - New `index_matches_commit_tree` to compare stage-0 index to a commit tree (like `diff-index --cached <tree>` empty).
   - When index differs from `HEAD` but already matches the merge target tree, skip the “dirty index” exit and skip `bail_if_merge_would_overwrite_local_changes` (work tree can still show `HEAD` content; t5402 setup).

3. **`GIT_DIR` without `GIT_WORK_TREE`** (`grit-lib/src/repo.rs`)
   - Documented that work tree defaults to **current working directory** (not parent of `.git`), matching Git for `GIT_DIR=other/.git` from a parent directory.

4. **Hook argv0 must be absolute** (`grit-lib/src/hooks.rs`)
   - Replaced relative `.git/hooks/...` / `hooks/...` argv0 with `hooks_dir.join(hook_name)` (absolute after canonical `git_dir`).
   - Fixes `spawn` failing when `current_dir` is the work tree but the hook path was relative to `.git`.

## Validation

- `./scripts/run-tests.sh t5402-post-merge-hook.sh` → 7/7
- `cargo test -p grit-lib --lib` → pass

## Note

`cargo clippy -- -D warnings` on the full workspace reports many pre-existing issues; not used as gate here.
