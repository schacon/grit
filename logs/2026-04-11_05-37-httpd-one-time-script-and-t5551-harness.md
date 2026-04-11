# 2026-04-11 05:37 — one_time_script route parity + t5551 harness enablement

## Summary

- Fixed `test-httpd` one-time-script smart-HTTP routing so repositories created outside `httpd/www` (in the test trash root) are still resolvable for CGI execution.
- Enabled `t5551-http-fetch-smart` in `data/test-files.csv` (`in_scope=yes`) and refreshed harness/dashboard state.

## Code changes

- `grit/src/bin/test_httpd.rs`
  - In `run_smart_http_cgi_output(...)`, added one-time-script fallback root selection:
    - default: `GIT_PROJECT_ROOT=$HTTPD_DOCUMENT_ROOT_PATH` (`config.root`)
    - for `/one_time_script` only:
      - if repo path is not under docroot and is under parent test root, use that parent test root as `GIT_PROJECT_ROOT`.
  - Added helper `repo_exists_under_root(root, smart_path)` for safe repo root probing.
  - Updated `PATH_TRANSLATED`/`GIT_PROJECT_ROOT` to use computed root.

- `data/test-files.csv`
  - `t5551-http-fetch-smart`: switched from `skip` to `yes`.

## Validation

- Quality gates:
  - `cargo fmt` ✅
  - `cargo check -p grit-rs` ✅
  - `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` ✅
  - `cargo test -p grit-lib --lib` ✅
  - `cargo build --release -p grit-rs` ✅

- Focused behavior checks:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=84 -v` ✅
    - expected message observed: `server does not support wait-for-done`.
  - `./scripts/run-tests.sh --timeout 240 t5551-http-fetch-smart.sh` ✅ (`31/37`, non-expected-failure tests pass)

- Full fetch-plan matrix rerun (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → `0/0` (timeout mode at default 30s)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → `31/37`
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `16/16`
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `37/37`
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → `16/16`
  - `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`
