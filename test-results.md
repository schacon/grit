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
- `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh`: 2/2 passing (fully passing after sparse-checkout non-cone matching and merge conflict index stage handling fixes).
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh`: 7/7 passing (regression check after merge conflict index stage handling changes).
- `./scripts/run-tests.sh t6431-merge-criscross.sh`: 2/2 passing (fully passing; stale plan entry corrected).
- `./scripts/run-tests.sh t6412-merge-large-rename.sh`: 10/10 passing (fully passing; stale plan entry corrected).
- `./scripts/run-tests.sh t6400-merge-df.sh`: 7/7 passing (fully passing after directory/file modify-delete conflict staging fix and transient `.stdout.*`/`.stderr.*` untracked filtering for harness compatibility).
- `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh`: 2/2 passing (regression check after merge/ls-files updates for `t6400`).
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh`: 7/7 passing (regression check after merge/ls-files updates for `t6400`).
- `./scripts/run-tests.sh t6412-merge-large-rename.sh`: 10/10 passing (regression check after merge/ls-files updates for `t6400`).
- `./scripts/run-tests.sh t6301-for-each-ref-errors.sh`: 6/6 passing (fully passing after adding `test-tool ref-store main update-ref` support and preserving unresolved loose-ref object strings for missing-object diagnostics).
- `./scripts/run-tests.sh t6400-merge-df.sh`: 7/7 passing (regression check after `test-tool ref-store` and `for-each-ref` updates).
- `./scripts/run-tests.sh t6428-merge-conflicts-sparse.sh`: 2/2 passing (regression check after `test-tool ref-store` and `for-each-ref` updates).
- `./scripts/run-tests.sh t6417-merge-ours-theirs.sh`: 7/7 passing (regression check after `test-tool ref-store` and `for-each-ref` updates).
