# t12160 cherry-pick rename merge path

## Problem

`tests/t12160-cherry-pick-conflict-resolve.sh` failed on "cherry-pick rename succeeds": picking a commit that renames `file.txt` → `renamed.txt` onto a branch that still had `file.txt` at the old path produced an "empty cherry-pick" because the merged tree matched HEAD (blob still at `file.txt`).

## Root cause

`merge_trees::three_way_on_aligned_paths` always wrote merged stage-0 entries under the **ours** path (`op`). When ours matched the merge base path but **theirs** had renamed that path, Git records the result at **their** destination path.

## Fix

In `grit-lib/src/merge_trees.rs`, when `bp == op` (no rename on our side relative to base) and `tp != op` (rename on their side), use `tp` as `out_path` for `merge_one_path`.

## Validation

- `./scripts/run-tests.sh t12160-cherry-pick-conflict-resolve.sh` → 34/34
- `cargo test -p grit-lib --lib`
