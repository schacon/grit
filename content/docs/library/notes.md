---
title: Notes
summary: Attach annotated notes to commits via RepositoryNotes and NotesRef.
---

Git notes are auxiliary commits under `refs/notes/*`. The [`notes`](rustdoc:grit_lib::notes) module defines [`NotesRef`](rustdoc:grit_lib::notes::NotesRef) (for example [`NotesRef::commits`](rustdoc:grit_lib::notes::NotesRef)) and read helpers. [`Repository::notes`](rustdoc:grit_lib::repo::Repository) exposes [`RepositoryNotes`](rustdoc:grit_lib::repo_handles::RepositoryNotes) for add, read, remove, and list operations with explicit committer/author options ([`NotesWriteOptions`](rustdoc:grit_lib::repo_handles::NotesWriteOptions)).

## Example

<!-- include: grit-examples/src/bin/guide_notes.rs -->

```bash
cargo run --bin guide_notes /path/to/repo
git notes show HEAD
```

The integration test compares the printed note body to `git notes show`.
