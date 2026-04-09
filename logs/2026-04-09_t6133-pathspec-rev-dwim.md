## t6133-pathspec-rev-dwim (2026-04-09)

- Ran `./scripts/run-tests.sh t6133-pathspec-rev-dwim.sh` → **6/6** passing on current branch.
- `data/test-files.csv` previously showed 5/6; refreshed to 6/6 after harness run.
- Removed unused `ZlibDecoder` / `Read` imports in `grit-lib` `odb` unit test module (clean `cargo test -p grit-lib --lib`).
