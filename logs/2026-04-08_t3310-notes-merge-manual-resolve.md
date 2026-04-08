# t3310-notes-merge-manual-resolve

## Goal

Make `tests/t3310-notes-merge-manual-resolve.sh` pass (22/22) with `grit` as `git`.

## Changes

- **`grit notes`**: Default notes ref from `core.notesRef`; `--ref z` → `refs/notes/z`; `-m` blobs end with newline like Git; `append` uses single `\n` between old and new paragraphs.
- **`notes merge`**: Manual strategy (default): merge-base + tree diffs, conflict files via `merge3`, `NOTES_MERGE_PARTIAL` / `NOTES_MERGE_REF` (symref) / `NOTES_MERGE_WORKTREE`; `--commit` / `--abort`; block concurrent merge into same ref when another is in progress; relative `.git/...` messages.
- **`grit-lib`**: `write_symbolic_ref`; store `NOTES_MERGE_*` under main `git_dir`; `rev-parse` tries `refs/notes/<short>`.
- **`update-ref`**: Resolve `refs/...` before ambiguous rev-parse; DWIM `refs/notes/`.
- **`log`**: Recursive notes tree load for fanout; `%N` in custom format strings.

## Verification

- `cargo test -p grit-lib --lib`
- `./scripts/run-tests.sh t3310-notes-merge-manual-resolve.sh` → 22/22
