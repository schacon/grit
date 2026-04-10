# t0602-reffiles-fsck

- Added `grit-lib::refs_fsck` implementing Git-style ref database checks (loose refs, symlink symrefs, packed-refs, worktrees, `fsck.*` severities).
- `grit refs verify` now delegates to `refs_fsck` for the files backend (reftable path unchanged).
- `grit fsck` gained `--[no-]references` and runs the same ref checks when references are enabled.
- `refs/worktree/*` writes go to the linked worktree admin dir (`ref_storage_dir`); `git worktree add` creates `refs/` under the admin dir.
- Fixed packed-refs parsing (single space after OID preserves leading spaces in refnames) and symlink target normalization when the target is missing.

Harness: `./scripts/run-tests.sh t0602-reffiles-fsck.sh` → 23/23 pass.
