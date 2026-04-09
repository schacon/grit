# t3009-ls-files-others-nonsubmodule

## Issue

`git ls-files -o` listed nested untracked repositories without a trailing `/` (e.g. `repo-no-commit-no-files` instead of `repo-no-commit-no-files/`).

## Root cause

`pathdiff_from_repo_for_display` used `work_tree.join(rel_str)` where `rel_str` included a trailing `/`. Rust `Path::join` normalizes that away, so the computed cwd-relative path lost the directory marker.

## Fix

Strip trailing `/` only for `Path::join`, then append `/` back to the result when the input was a directory marker.

## Verification

- `./scripts/run-tests.sh t3009-ls-files-others-nonsubmodule.sh` — pass (2/2)
- `./scripts/run-tests.sh t3005-ls-files-relative.sh` — pass (4/4)
- `cargo test -p grit-lib --lib` — pass
