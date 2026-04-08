# t5732-protocol-v2-bundle-uri-http

## Problem

Harness reported 6/9 passing. Failures were:

1. `git test-tool bundle-uri ls-remote <http-url>` was routed to system `git` by the hybrid wrapper in `lib-httpd.sh` (any argv containing `http://`), so `test-tool` was missing.
2. `test_uri_escape` and `test_cmp_config_output` were undefined in the slim `test-lib-harness.sh` (only in full `test-lib-functions.sh`).
3. `grit serve-v2` did not implement `bundle-uri` (upload-pack path used real git-http-backend → upstream git, which does implement it, but grit’s test-tool had no HTTP client).

## Changes

- **`serve_v2`**: `cmd_bundle_uri` reads repo `config` and emits sorted `bundle.*=value` pkt-lines + flush (matches upstream `bundle_uri_command`).
- **`http_bundle_uri.rs`**: Smart HTTP v2 client using `ureq` — GET `info/refs?service=git-upload-pack` with `Git-Protocol: version=2`, parse caps (skip v0 `# service=` block only when present), POST `git-upload-pack` with `command=bundle-uri` + capability echo, parse response into bundle list text for `test_cmp_config_output`.
- **`lib-httpd.sh`**: Hybrid `git` wrapper skips HTTP delegation when argv contains `test-tool` and `bundle-uri`.
- **`test-lib-harness.sh`**: Added `test_uri_escape` and `test_cmp_config_output`.
- **`tests/test-tool`**: Delegate `bundle-uri` to grit binary.

## Verification

`./scripts/run-tests.sh t5732-protocol-v2-bundle-uri-http.sh` → 9/9.
