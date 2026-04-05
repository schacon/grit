# Test Results

**Updated:** 2026-04-05

- `cargo test --workspace`: not run for this task.
- `./tests/harness/run.sh`: not run for this task.
- `./scripts/run-tests.sh t6414-merge-rename-nocruft.sh`: 3/3 passing (now fully passing).
- `./scripts/run-tests.sh t6408-merge-up-to-date.sh`: 7/7 passing (fully passing).
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh`: 7/7 passing (now fully passing).
- `./scripts/run-tests.sh t6110-rev-list-sparse.sh`: 2/2 passing (fully passing; stale plan entry corrected).
- `./scripts/run-tests.sh t6425-merge-rename-delete.sh`: 1/1 passing (fully passing; stale plan entry corrected).
- `./scripts/run-tests.sh t6114-keep-packs.sh`: 3/3 passing (fully passing; stale plan entry corrected).
- `./scripts/run-tests.sh t6134-pathspec-in-submodule.sh`: 3/3 passing (fully passing after unpopulated submodule `git add -C` handling fix).
- `./scripts/run-tests.sh t6136-pathspec-in-bare.sh`: 3/3 passing (fully passing after bare-repo pathspec diagnostics fixes in `log` and `ls-files`).
