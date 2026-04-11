## Summary

Completed the next fetch-plan HTTP parity slice by fixing smart HTTP behavior for:

- `GIT_SMART_HTTP=0` dumb-discovery fallback behavior in request paths/traces
- cookie loading + `GIT_TRACE_CURL` cookie redaction parity
- HTTP fetch-by-object-id refspec handling over smart transport
- no-op HTTP fetch short-circuiting when all wants are already local

This closes the remaining non-TODO failures in `t5551-http-fetch-smart.sh`.

## Changes implemented

### 1) HTTP client mode + cookie support

- File: `grit/src/http_client.rs`
- Changes:
  - Added `smart_http_enabled` derived from `GIT_SMART_HTTP` (`0` disables smart endpoint usage).
  - Added `cookie_header` derived from `http.cookieFile` config.
  - Request builders now attach `Cookie:` for GET/POST across transport backends.
  - Added cookie trace output with redaction parity:
    - default: `Foo=<redacted>; Bar=<redacted>`
    - `GIT_TRACE_REDACT=0`: raw cookie values.
  - Added URL rewriting helper for disabled smart mode so discovery/request paths become
    dumb `.../info/refs` instead of `.../info/refs?service=...` and RPC endpoints.

### 2) HTTP smart wants/refspec parity improvements

- File: `grit/src/http_smart.rs`
- Changes:
  - `collect_wants_from_advertised(...)` now supports source refspecs that are literal object IDs
    (SHA wants) and pushes them directly into wants.
  - Added advertised-ref source resolver so short names resolve consistently (`refs/*`, tags,
    heads, remotes) instead of hardcoding `refs/heads/<src>`.
  - Added no-op short-circuit for both v0/v1 and v2 HTTP fetch paths:
    - if `--refetch` is not set,
    - no shallow/filter extensions are requested,
    - and all wants already exist locally,
    - skip POST pack negotiation and return advertised refs directly.

## Validation performed

- Build/quality:
  - `cargo fmt`
  - `cargo check -p grit-rs`
  - `cargo test -p grit-lib --lib`
  - `cargo build --release -p grit-rs`
- Focused HTTP suite:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5551-http-fetch-smart.sh -v`
  - Result: all non-TODO tests pass; remaining failures are expected `test_expect_failure` TODOs.
- Matrix checkpoint (ordered):
  1. `./scripts/run-tests.sh t5702-protocol-v2.sh` → `0/0`
  2. `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning in current harness selection
  3. `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
  4. `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
  5. `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `16/16`
  6. `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `37/37`
  7. `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → `16/16`
  8. `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`

## Result

`t5551-http-fetch-smart.sh` non-TODO failures are resolved in manual full-suite execution,
and the fetch-plan matrix remains green across the tracked harness suites.
