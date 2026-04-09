# t3303-notes-subtrees

## Failures

- `grit fast-import` rejected `M 644 inline <path>` and `data <<DELIM` heredoc streams used by the test setup and note imports.
- After fixing import, `grit log` showed wrong notes when the notes tree had two blob paths for the same commit (duplicate vs concatenated cases).

## Fixes

- **grit-lib `fast_import.rs`**: `read_data_payload` for `data <<term` heredoc and byte-count payloads; use it for blob and commit messages; handle `M <mode> inline <path>`; support `deleteall` during `finish_commit`.
- **grit `log.rs`**: when building the notes map from the tree, merge multiple blobs for the same commit OID using Git’s `combine_notes_concatenate` rules; skip re-merge when blob bytes are identical (same as Git when OIDs match).

## Verification

- `./scripts/run-tests.sh t3303-notes-subtrees.sh` — 23/23
- `cargo test -p grit-lib --lib` — pass
