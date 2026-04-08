# t4007-rename-3 — diff-files copy detection

## Failures

- `git diff-files -C --find-copies-harder -R` returned `unsupported option: -C`.
- After wiring options, raw output showed `C path0 path0` instead of `C path1 path0` (wrong copy pairing when multiple index entries shared the same blob).

## Fixes

1. **`grit/src/commands/diff_files.rs`**: Parse `-M`, `-C`, `--find-copies-harder`, `-R`; build `DiffEntry` list with real worktree OIDs; reverse entries before `detect_copies` when `-R`; run `detect_copies` with index stage-0 paths as `find_copies_harder` sources; raw output uses real new OID when reversed (matches Git).
2. **`grit-lib/src/diff.rs`**: In `detect_copies`, skip score entries where source path equals the added path (Git does not treat a path as a copy of itself).

## Verification

- `./scripts/run-tests.sh t4007-rename-3.sh` → 13/13
- `cargo test -p grit-lib --lib`
