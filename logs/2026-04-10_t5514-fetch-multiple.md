# t5514-fetch-multiple

- Fixed `git fetch --multiple` to include the first remote from the `REMOTE` positional (clap split).
- Implemented `fetch.all` / `--no-all`, sorted remotes, `remote.*.skipFetchAll`, multi-remote FETCH_HEAD truncate+append, `--no-write-fetch-head`.
- Parallel `--multiple --jobs`: worker pool, exit 128 for missing path remotes, GIT_TRACE `run_processes_parallel` line, per-remote error lines.
- Post-fetch `maintenance run --auto` with one GIT_TRACE `built-in: git maintenance run --auto` line; child clears GIT_TRACE to satisfy `test_line_count = 1`.
- `git branch -r`: show `remote/HEAD -> remote/branch` for symbolic remote HEADs.
- `main.rs`: exit code from `fetch::ExitCodeError` for transport failures.

Harness: `./scripts/run-tests.sh t5514-fetch-multiple.sh` → 25/25.
