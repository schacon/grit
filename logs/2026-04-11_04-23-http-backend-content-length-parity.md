## Summary

Completed `grit http-backend` CGI implementation for smart HTTP content-length and gzip
handling, bringing `t5562-http-backend-content-length.sh` to full pass.

## Changes implemented

### 1) Implemented smart HTTP CGI request routing and service execution

- File: `grit/src/commands/http_backend.rs`
- Added:
  - service detection for upload-pack / receive-pack from CGI env
  - request method dispatch (`GET`, `POST`)
  - repository path derivation from `PATH_TRANSLATED`
  - child process execution for:
    - `grit upload-pack <repo>`
    - `grit receive-pack <repo>`
    - `grit upload-pack <repo> --advertise-refs` for discovery

### 2) Added robust body/encoding/content-length handling

- File: `grit/src/commands/http_backend.rs`
- Added:
  - `CONTENT_LENGTH` parsing with overflow checks
  - exact body reads for explicit length and fallback streaming reads
  - gzip decode support via `HTTP_CONTENT_ENCODING` / `CONTENT_ENCODING`
  - protocol payload validation for upload-pack and receive-pack requests

### 3) Added CGI response writer with Git-compatible 200 behavior

- File: `grit/src/commands/http_backend.rs`
- Added:
  - normalized response object with status/content-type/body
  - CGI header/body emission with `Content-Type` + `Content-Length`
  - `Status:` header emitted only for non-`200 OK` responses (Git parity)

## Validation performed

- `cargo fmt`
- `cargo check -p grit-rs`
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` (reverted unrelated clippy edit)
- `cargo test -p grit-lib --lib`
- `cargo build --release -p grit-rs`
- Focused suite:
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **16/16**
- Matrix checkpoint (ordered):
  1. `./scripts/run-tests.sh t5702-protocol-v2.sh` → `0/0`
  2. `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning in this harness selection
  3. `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
  4. `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
  5. `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `16/16`
  6. `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `37/37`
  7. `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **`16/16`**
  8. `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`

## Result

`t5562-http-backend-content-length.sh` is now fully green in this environment, and the
fetch-plan matrix checkpoints remain stable.
