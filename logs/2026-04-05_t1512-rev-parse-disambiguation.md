# t1512-rev-parse-disambiguation

- Read `AGENTS.md`, `plan.md`, and `git/t/t1512-rev-parse-disambiguation.sh`.
- Reproduced the scoped failures and confirmed the missing behavior was Git-style ambiguity reporting for abbreviated object IDs involving ambiguous blobs, invalid loose object types, and corrupt zlib loose objects.
- Patched the rev-parse ambiguity path in Rust so `rev-parse` now prints Git-compatible candidate lists and fatal diagnostics for those three cases.
- Rebuilt with `CARGO_TARGET_DIR=/tmp/grit-build-t1512 cargo build --release -p grit-rs`.
- Verified the scoped upstream behavior with `CARGO_TARGET_DIR=/tmp/grit-build-t1512 bash scripts/run-upstream-tests.sh t1512 2>&1 | tail -40` and direct spot checks of the three failing cases.
- Verified the tracked local slice with `cd tests && GUST_BIN="$(pwd)/grit" BASH_ENV=<temp env sourcing tests/test-lib-functions.sh with test_hash_algo=sha1> bash t1512-rev-parse-disambiguation.sh`: 3/3 passing.
- Updated `plan.md`, `progress.md`, `test-results.md`, and `data/file-results.tsv` for the completed task.
