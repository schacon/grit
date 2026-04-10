## Phase
- Phase E.4 harness parity support (`one_time_script` route in local `test-httpd`).

## Motivation
- `t5702` negotiate-only case `--run=84` still failed in this environment before reaching
  wait-for-done assertions because the local test HTTP server returned:
  - `fatal: repository 'http://127.0.0.1:<port>/one_time_script/server/' not found`
- Upstream tests expect `/one_time_script/<repo>` to execute `$HTTPD_ROOT_PATH/one-time-script`
  once against CGI pkt output before returning a response.

## Code changes
- Updated `grit/src/bin/test_httpd.rs`:
  - Added route handling for `/one_time_script/`.
  - Added helper to locate script path: `<docroot parent>/one-time-script`.
  - Refactored smart CGI execution into reusable helper returning raw CGI output.
  - Added one-time script execution flow:
    - write CGI output to temp file
    - invoke `one-time-script <tempfile>`
    - capture transformed stdout
    - delete temp file
    - delete one-time script after use (one-shot semantics)
    - parse/send transformed CGI response.

## Validation
- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=83,84,85`
  - `83` pass
  - `85` pass
  - `84` still fails in this environment, but now consistently in setup path (`test_commit` no-op
    and downstream clone path assumptions), not due to missing `/one_time_script` route support.

## Notes
- This is a harness parity improvement (test infra behavior) and does not alter fetch wire logic.
