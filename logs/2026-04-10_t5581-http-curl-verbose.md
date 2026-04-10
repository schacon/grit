# t5581-http-curl-verbose

## Symptom

Harness reported 1/2: second test greps `curl_log` for libcurl-style
`<= Recv header: HTTP/1.1 500 Intentional Breakage` after cloning
`$HTTPD_URL/error_git_upload_pack/smart/repo.git` with `GIT_CURL_VERBOSE=1`.

HTTP clone uses system `git` (hybrid wrapper in `tests/lib-httpd.sh`); failure was in **test-httpd**, not grit HTTP client.

## Root cause

`grit/src/bin/test_httpd.rs` did not implement Apache’s `ScriptAliasMatch` for
`/error_git_upload_pack/(.*)/git-upload-pack` → `error.sh`, and did not strip the
`/error_git_upload_pack` prefix for routing, so `/error_git_upload_pack/smart/...`
never hit the `/smart/` git-http-backend path.

## Fix

- `routing_path()`: strip `/error_git_upload_pack` so paths route like `/smart/repo.git/...`.
- Early handler: for original path under `/error_git_upload_pack/`, POST to `.../git-upload-pack` → HTTP 500 `Intentional Breakage` + plain body (matches upstream `error.sh`).
- `handle_smart_http_with_path`: use `routing_path(&req.path)` when computing `PATH_INFO` / `PATH_TRANSLATED`.

## Verification

```bash
./scripts/run-tests.sh t5581-http-curl-verbose.sh
# ✓ 2/2
```

## Commit

(fix committed on branch `cursor/-bc-884eadf4-465b-4312-8f23-c1a91cb7c3e4-40ee`)
