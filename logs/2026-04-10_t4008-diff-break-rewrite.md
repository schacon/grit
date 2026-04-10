# t4008-diff-break-rewrite

## Goal

Make `tests/t4008-diff-break-rewrite.sh` pass (14/14): `diff-index -B`, `-B -M`, `-B -C`, typechange + copy pairing, uncached rename/copy with worktree placeholder OIDs.

## Changes

### `grit-lib` (`diff.rs`)

- `detect_renames(odb, work_root, …)` / `detect_copies(odb, work_root, …)`: optional worktree root to read added-side bytes when `new_oid` is zero (uncached `diff-index`).
- Skip line-similarity rename pairing across non-regular modes (regular ↔ symlink).
- `detect_copies`: include `TypeChanged` as copy sources; post-pass to turn `Modified` into `Copied` when new content matches a non-deleted source (symlink preimage → sibling file).
- Public helpers: `parse_diff_rename_score_token`, `should_break_rewrite_pair`, `GIT_DIFF_*` score constants.

### `grit` (`diff_index.rs`)

- Parse `-B` / `--break-rewrites[=<n>[/<m>]]`.
- `diff_tree_vs_index`: fix typechange detection (compare `mode & 0170000`, not full `100644`).
- `diff_tree_vs_worktree`: set status `T` when tree vs worktree types differ after worktree refresh.
- `apply_diffcore_break_rewrites_split` + `merge_broken_rewrite_pairs` + `drop_break_delete_superseded_by_rename_dest`.
- When `-B` and `-M`: run `detect_copies` after `detect_renames` (Git combines break + rename + copy detection).
- Raw output: for uncached `A`, show worktree blob hash on new side when non-empty and not empty-blob (t4008 #8 vs t1501 empty add).
- `T` raw lines with optional dissimilarity score when `-B`.

### Call-site updates

All `detect_renames` / `detect_copies` call sites pass `None` for `work_root` except `diff-index` (uses repo work tree when uncached).

## Verification

- `./scripts/run-tests.sh t4008-diff-break-rewrite.sh` → 14/14
- `cargo test -p grit-lib --lib`
