# t5514-fetch-multiple

**Date:** 2026-04-09

## Outcome

`./scripts/run-tests.sh t5514-fetch-multiple.sh` reports **25/25** passing on current `grit` (branch `cursor/t5514-fetch-multiple-tests-58ba`). No Rust code changes were required in this session; the harness run refreshed `data/test-files.csv` and dashboards.

## Coverage (from upstream test file)

- `git fetch --all` across multiple remotes; `--no-write-fetch-head`
- Continue after a bad remote; reject extra arguments with `--all`
- `git fetch --multiple` (one/two remotes, bad remote names)
- `remote.<name>.skipFetchAll` vs explicit `--multiple`
- `--all --no-tags` / `--all --tags`
- Parallel `fetch --jobs=2 --multiple` (trace + error messages)
- `fetch.all` config, `git fetch` default vs explicit remote, `--no-all` interaction with `--all`

## Follow-up

None for this file.
