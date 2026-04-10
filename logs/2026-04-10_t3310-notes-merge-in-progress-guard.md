# t3310-notes-merge-manual-resolve — in-progress merge guard

## Problem

Tests 7 and 13 failed: starting a second `git notes merge` while conflict files still lived under `.git/NOTES_MERGE_WORKTREE` returned the generic “Automatic notes merge failed…” message instead of Git’s “previous notes merge (.git/NOTES_MERGE_* exists)” error. That also left `NOTES_MERGE_PARTIAL`/`NOTES_MERGE_REF` in a bad state so `git notes merge --commit` could not finalize.

## Fix

In `merge_one_note_change` for manual strategy, before creating the worktree for a **new** merge (`has_worktree` false), detect a non-empty existing `NOTES_MERGE_WORKTREE` (same idea as Git’s `check_notes_merge_worktree` + `is_empty_dir`) and bail with a message that includes `.git/NOTES_MERGE_* exists`.

## Verification

- `./scripts/run-tests.sh t3310-notes-merge-manual-resolve.sh` → 22/22
- `cargo test -p grit-lib --lib`
- `cargo fmt`, `cargo clippy --fix --allow-dirty -p grit-rs`
