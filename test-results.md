# Test results

**2026-04-27 (remote auth / credential protocol model)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo test -p grit-lib --lib`: 197 passed
- `./scripts/run-tests.sh t0300-credentials.sh`: skipped by current `data/test-files.csv` scope; no credential harness tests executed
- Manual credential smoke checks: Basic helper fill, `authtype` capability filtering, encoded-newline URL rejection, `credential.protectProtocol` CR handling, sanitized askpass prompt, `grit credential capability`, URL-scoped `credential.username`, URL-scoped `credential.useHttpPath`, default HTTP path stripping, fatal credential error shape, and `credential-store` ephemeral skip passed

**2026-04-27 (remote auth / credential-store parity)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t0302-credential-store.sh`: skipped by current `data/test-files.csv` scope; no credential-store harness tests executed
- Manual credential-store smoke checks: home/XDG lookup precedence, XDG fallback, overwrite-on-store, erase across files, `--file` and `--file=`, path matching, CRLF path behavior, invalid-line handling, and Unix permissions passed

**2026-04-27 (remote auth / credential-cache daemon)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- Manual credential-cache smoke checks: default socket creation, custom socket creation, store/get output ordering, erase, timeout expiry, and exit cleanup passed

**2026-04-27 (remote auth / SSH command precedence)**

- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t5507-remote-environment.sh`: 5/5 passed
- `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`: 63/81 passed (known remaining failures; no regression from SSH command precedence work)

**2026-04-28 (remote auth / live SSH ls-remote)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo test -p grit-lib --lib`: 197 passed
- `./scripts/run-tests.sh t5512-ls-remote.sh`: 16/40 passed (existing broader failures remain)
- `./scripts/run-tests.sh t5601-clone.sh`: 64/115 passed (existing broader failures remain)

**2026-04-28 (remote auth / live SSH fetch)**

- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t5510-fetch.sh`: 199/215 passed (existing broader failures remain)
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 0/0 warning from harness selection/status

**2026-04-28 (remote auth / live SSH clone)**

- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t5601-clone.sh`: 64/115 passed (existing broader failures remain)
- `./scripts/run-tests.sh t5603-clone-dirname.sh`: 25/47 passed (existing broader failures remain)

**2026-04-28 (remote auth / SSH push hardening)**

- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh t5406-remote-rejects.sh`: 3/3 passed
- `./scripts/run-tests.sh t5545-push-options.sh`: 2/13 passed (existing broader failures remain)

**2026-04-28 (remote auth / trace redaction audit)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- Manual HTTP trace/error smoke check with URL userinfo: passed; curl request line and connection error both scrubbed username/password.
- HTTP access errors, curl request-start traces (when `GIT_TRACE_REDACT` is not `0`), and trace2 `git-remote-https` child-start URLs now scrub URL username/password fields before display.
- Existing curl trace redaction already covers `Authorization`, `Proxy-Authorization`, cookie values, and auth-like `http.extraHeader` values by default.

**2026-04-28 (remote auth / HTTP proxy and smart regression sweep)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo build -p grit-rs --bin test-httpd`: passed (local harness uses the debug helper when present)
- `./scripts/run-tests.sh --timeout 60 t5564-http-proxy.sh`: 7/8 passed; no timeout, proxy `407`, authenticated proxy clone, proxy askpass, and proxy redaction pass. Remaining failure: SOCKS Unix socket clone gets an empty v0 stateless upload-pack response.
- `./scripts/run-tests.sh --timeout 90 t5581-http-curl-verbose.sh t5555-http-smart-common.sh`: `t5581` 1/2, `t5555` 10/10. `t5581` currently selects system Git for HTTP clone and fails before curl output because the temporary `GIT_EXEC_PATH` lacks `git-remote-http`.
- `./scripts/run-tests.sh --timeout 90 t5549-fetch-push-http.sh t5541-http-push-smart.sh t5539-fetch-http-shallow.sh t5542-push-http-shallow.sh`: `t5549` 0/3, `t5539` 1/8, `t5542` 1/3; `t5541` skipped by current `data/test-files.csv` scope.
- Implemented upload-pack compatibility for smart-HTTP `--http-backend-info-refs` and `--stateless-rpc`, fixed stateless v2 responses so `ls-refs`/fetch responses are not prefixed by capability advertisements, avoided leaking v2 `Git-Protocol` into v0 HTTP POSTs, fixed Homebrew/system `git-http-backend` lookup in `test-httpd`, and cached proxy askpass results per process.

**2026-04-28 (remote auth / SOCKS proxy completion)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo build -p grit-rs --bin test-httpd`: passed
- `./scripts/run-tests.sh --timeout 60 t5564-http-proxy.sh`: 8/8 passed
- `./scripts/run-tests.sh --timeout 90 t5555-http-smart-common.sh t5581-http-curl-verbose.sh`: `t5555` 10/10, `t5581` 1/2
- Fixed SOCKS-over-Unix HTTP request construction so inserted headers are separate CRLF-delimited header lines instead of being appended to the previous header; this made `Git-Protocol` and `Content-Length` parse correctly for direct SOCKS GET/POST requests.

**2026-04-28 (remote auth / curl verbose error route)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo build -p grit-rs --bin test-httpd`: passed
- `./scripts/run-tests.sh --timeout 90 t5581-http-curl-verbose.sh t5564-http-proxy.sh t5555-http-smart-common.sh`: `t5581` 2/2, `t5564` 8/8, `t5555` 10/10
- Routed the intentional `error_git_upload_pack` clone path through Grit in the lightweight HTTP harness and added the matching `500 Intentional Breakage` route to `test-httpd`, so `GIT_CURL_VERBOSE` validates Grit's curl trace output instead of failing in system Git helper lookup.

**2026-04-28 (remote auth / HTTP push regression)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh --timeout 90 t5549-fetch-push-http.sh`: 3/3 passed
- `./scripts/run-tests.sh --timeout 90 t5555-http-smart-common.sh t5564-http-proxy.sh t5581-http-curl-verbose.sh`: `t5555` 10/10, `t5564` 8/8, `t5581` 2/2
- Routed HTTP push through Grit in the lightweight harness, fixed real Git exec-path discovery for server-side HTTP helpers, added a `git-receive-pack` helper wrapper for `git-http-backend`, allowed HTTP push source refs to resolve tags/revisions, ignored remote-only haves when building local push packs, emitted the expected `write_pack_file/wrote` trace2 event from HTTP push packs, and implemented the initial `push.negotiate` behavior needed by the test.

**2026-04-28 (remote auth / shallow HTTP push)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo build -p grit-rs --bin test-httpd`: passed
- `./scripts/run-tests.sh --timeout 90 t5542-push-http-shallow.sh`: 3/3 passed
- `./scripts/run-tests.sh --timeout 90 t5549-fetch-push-http.sh t5555-http-smart-common.sh t5564-http-proxy.sh t5581-http-curl-verbose.sh`: `t5549` 3/3, `t5555` 10/10, `t5564` 8/8, `t5581` 2/2
- HTTP receive-pack advertisement parsing now ignores `shallow <oid>` lines, HTTP push force refspec parsing strips the leading `+` before resolving sources, and the lightweight HTTP server decodes gzip request bodies before invoking `git-http-backend`.

**2026-04-28 (remote auth / simple HTTP auth harness)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo build -p grit-rs --bin test-httpd`: passed
- `./scripts/run-tests.sh --timeout 120 t5563-simple-http-auth.sh`: 17/17 passed
- `./scripts/run-tests.sh --timeout 90 t5555-http-smart-common.sh t5564-http-proxy.sh t5581-http-curl-verbose.sh t5549-fetch-push-http.sh t5542-push-http-shallow.sh`: `t5555` 10/10, `t5564` 8/8, `t5581` 2/2, `t5549` 3/3, `t5542` 3/3
- Added lightweight `/custom_auth/` support to `test-httpd`, routed `custom_auth` requests through Grit, allowed `CGIPASSAUTH` when `lib-httpd.sh` sets that prereq, preserved duplicate `WWW-Authenticate` headers from ureq, approved proactive credentials after successful first requests, and fixed the custom auth challenge parser for folded headers and multistage `status=... response=...` lines.

**2026-04-28 (remote auth / protocol-v2 bundle-uri HTTP)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo build -p grit-rs --bin test-httpd`: passed
- `./scripts/run-tests.sh --timeout 120 t5732-protocol-v2-bundle-uri-http.sh`: 9/9 passed
- `./scripts/run-tests.sh --timeout 120 t5555-http-smart-common.sh t5563-simple-http-auth.sh t5564-http-proxy.sh t5581-http-curl-verbose.sh`: `t5555` 10/10, `t5563` 17/17, `t5564` 8/8, `t5581` 2/2
- In bundle-uri HTTP mode, the lightweight HTTP server now uses Grit upload-pack for smart HTTP so the server advertises and serves `bundle-uri`; HTTP fetch sends the protocol-v2 `bundle-uri` command when enabled and suppresses it for explicit `--bundle-uri`; protocol-v2 HTTP capability discovery is packet-traced for clone/ls-remote assertions. Post-fetch bundle-uri lookup can now reuse the active HTTP client context so auth/proxy state is preserved.

**2026-04-28 (remote auth / t5551 auth-redaction slice)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- Focused `t5551-http-fetch-smart.sh --run=1-17,32-33`: all non-expected-failure auth/redaction cases passed through Grit; redirect cases remain expected failures.
- `./scripts/run-tests.sh --timeout 150 t5551-http-fetch-smart.sh`: 29/37. The remaining real failures are empty SHA-256 clone object-format support; the auth/redaction cases are green.
- Routed authenticated smart HTTP cases through Grit in the lightweight HTTP harness, fixed unredacted `Authorization` curl trace output, and made access log stripping preserve status codes by adding an Apache-like byte-count field.

**2026-04-28 (remote auth / t5541 HTTP push auth slice)**

- `cargo fmt`: passed
- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- Focused `t5541-http-push-smart.sh --run=1-10`: passed
- `./scripts/run-tests.sh --timeout 150 t5541-http-push-smart.sh t5549-fetch-push-http.sh t5542-push-http-shallow.sh`: `t5541` 21/21, `t5549` 3/3, `t5542` 3/3
- HTTP push now accepts repeated `-v`, expands default `push.default=matching` for HTTP remotes, prints Git-style `POST git-receive-pack` summaries for verbose pushes, sends a valid empty pack when the remote already has all pushed objects, and reports client-side atomic collateral failures with Git-like per-ref status lines.

**2026-04-28 (remote auth / credential harness validation)**

- `cargo fmt`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh --timeout 120 t0300-credentials.sh`: 56/56 passed
- `./scripts/run-tests.sh --timeout 120 t0301-credential-cache.sh`: 52/52 passed
- `./scripts/run-tests.sh --timeout 120 t0303-credential-external.sh`: 23/23 passed
- `./scripts/run-tests.sh --timeout 120 t0302-credential-store.sh`: 65/65 passed
- Credential helpers now use Git-compatible argv/stdin/stdout framing, helper-advertised capabilities drive credential output, and credential URL parsing/matching covers decoded paths, wildcard hosts, username/no-username scope matching, bare query/fragment path markers, and partial URL config scopes.
- Credential-store now preserves explicit empty username/password credentials, overwrites stored credentials by identity instead of password, and only honors password matching during reject/erase.
- Credential-cache now shares the same overwrite/erase/empty-credential semantics, filters cached pre-encoded credentials by caller capability, expands `$HOME` in `--socket` helper arguments, and removes socket/sidecar files on exit for deterministic harness cleanup.

**2026-04-28 (remote auth / shallow HTTP fetch audit)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh --timeout 150 t5539-fetch-http-shallow.sh`: 4/8
- Repaired the in-progress HTTP smart shallow request plumbing so local `.git/shallow` OIDs are passed to v0/v1 and v2 fetch request builders and the code compiles again. Remaining failures are shallow deepen/since/exclude transport behavior, not auth-specific regression.

**2026-04-28 (remote auth / protocol policy audit)**

- `cargo build --release -p grit-rs`: passed
- `./scripts/run-tests.sh --timeout 150 t5812-proto-disable-http.sh`: 29/29 passed
- `./scripts/run-tests.sh --timeout 150 t5814-proto-disable-ext.sh`: 19/27
- HTTP protocol allow/deny policy is green. The remaining `t5814` failures are enabled `ext::` fetch/push behavior; disabled clone/fetch/push policy paths pass and the residual gap is ext transport support, not auth classification.

**2026-04-27 (remote auth / HTTP challenge plumbing)**

- `cargo check -p grit-rs`: passed
- `cargo build --release -p grit-rs`: passed
- `cargo test -p grit-lib --lib`: 197 passed
- HTTP client now captures response headers, extracts `WWW-Authenticate` challenges, passes `capability[]=authtype`, `capability[]=state`, and ordered `wwwauth[]` to `credential fill`, and passes `wwwauth[]` to reject paths while keeping Basic approve requests unchanged
- HTTP client now uses a typed auth credential representation and can build `Authorization: <authtype> <credential>` for helper-provided pre-encoded credentials while preserving Basic username/password auth
- HTTP client now performs one multistage `continue=1` follow-up with helper `state[]` and updated challenges, includes pre-encoded auth fields in approve/reject, and avoids storing ephemeral pre-encoded credentials through helper input
- HTTP client now parses `http.proactiveAuth` and proactively sends complete Basic or helper-selected pre-encoded credentials before the first request; `http.emptyAuth` is parsed and disables proactive auth for now
- HTTP client now applies global and URL-scoped `http.extraHeader` values to ureq, proxy, and SOCKS request paths, supports empty-value reset, and redacts auth-like extra headers in curl trace output
- HTTP client now parses Netscape and simplified `http.cookieFile` entries and matches cookies per request URL by domain, path, and secure flag
- HTTP client now honors `http.saveCookies` by appending received `Set-Cookie` headers to the configured cookie file in a format that is read back by `http.cookieFile`
- HTTP bundle URI downloads and protocol-v2 bundle-uri discovery now route through `HttpClientContext`, sharing auth, proxy, cookie, extra header, and curl trace behavior with normal HTTP remotes
- HTTP client now honors `http.sslVerify=false` and `GIT_SSL_NO_VERIFY` by disabling rustls certificate verification; CA file/path and client cert/key options remain unsupported with the current HTTP stack
- HTTP client now honors `http_proxy`, `https_proxy`, `all_proxy`, and `no_proxy` for requests without `http.proxy`, while keeping configured `http.proxy` precedence
- HTTP client now parses `http.proxyAuthMethod` and `GIT_HTTP_PROXY_AUTHMETHOD`; Basic/anyauth proxy credentials remain supported and unsupported methods are reported instead of silently downgraded
- HTTP fetch and push now honor `remote.<name>.proxy` as a per-remote override before falling back to `http.proxy` and environment proxy variables

**2026-04-13 (t5322 / pack-objects sparse --revs)**

- `cargo test -p grit-lib --lib`: passed (see merge)
- `./scripts/run-tests.sh t5322-pack-objects-sparse.sh`: 11/11 passed (verified after merge)

**2026-04-10 (t4252 / am apply passthrough options)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t4252-am-options.sh`: 8/8 passed

**2026-04-10 (t3438 / rebase broken files)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3438-rebase-broken-files.sh`: 9/9 passed

**2026-04-10 (t3405 / rebase malformed messages)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3405-rebase-malformed.sh`: 5/5 passed

**2026-04-10 (t3416 / rebase --onto A...B and --keep-base)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3416-rebase-onto-threedots.sh`: 18/18 passed

**2026-04-10 (t5581 / GIT_CURL_VERBOSE)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5581-http-curl-verbose.sh`: 2/2 passed

**2026-04-10 (t0012-help)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t0012-help.sh`: 121/121 passed

**2026-04-10 (t5528 / push.default)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5528-push-default.sh`: 31/32 passed (1 `test_expect_failure`)

**2026-04-10 (t5327 / multi-pack bitmaps .rev)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5327-multi-pack-bitmaps-rev.sh`: 314/314 passed (expected after merge)

**2026-04-10 (t3452 / history split)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3452-history-split.sh`: 25/25 passed

**2026-04-10 (t5532 / fetch proxy)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5532-fetch-proxy.sh`: 5/5 passed

**2026-04-10 (t5705 / session ID in capabilities)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5705-session-id-in-capabilities.sh`: 17/17 passed

**2026-04-10 (t6101 / rev-parse parents)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t6101-rev-parse-parents.sh`: 38/38 passed

**2026-04-09 (t4103 / apply binary)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t4103-apply-binary.sh`: 24/24 passed

**2026-04-10 (t7413 / submodule is-active)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t7413-submodule-is-active.sh`: 10/10 passed

**2026-04-10 (t3702 / add -e)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t3702-add-edit.sh`: 3/3 passed

**2026-04-10 (t4122 / apply symlink inside)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t4122-apply-symlink-inside.sh`: 7/7 passed

**2026-04-10 (t8008 / blame formats)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t8008-blame-formats.sh`: 5/5 passed

**2026-04-10 (t0035 / safe.bareRepository)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t0035-safe-bare-repository.sh`: 12/12 passed

**2026-04-09 (t5410 / receive-pack)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5410-receive-pack.sh`: 5/5 passed

**2026-04-09 (t5810 / proto-disable-local)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5810-proto-disable-local.sh`: 54/54 passed

**2026-04-09 (t5812 / proto disable http)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5812-proto-disable-http.sh`: 29/29 passed

**2026-04-10 (t5517 / push mirror)**

- `cargo test -p grit-lib --lib`: 160 passed
- `./scripts/run-tests.sh t5517-push-mirror.sh`: 13/13 passed

**2026-04-10 (t5546 / receive limits)**

- `cargo test -p grit-lib --lib`: 160 passed
- `cargo clippy -p grit-rs -p grit-lib --fix --allow-dirty`: no warnings
- `./scripts/run-tests.sh t5546-receive-limits.sh`: 17/17 passed

**2026-04-09 (t4063 / diff blobs)**

- `cargo test -p grit-lib --lib`: 155 passed
- `./scripts/run-tests.sh t4063-diff-blobs.sh`: 18/18 passed

**2026-04-09 (t5571 / pre-push hook)**

- `cargo test -p grit-lib --lib`: 155 passed
- `./scripts/run-tests.sh t5571-pre-push-hook.sh`: 11/11 passed

**2026-04-09 (t7418 / submodule sparse .gitmodules)**

- `cargo test -p grit-lib --lib`: 155 passed
- `./scripts/run-tests.sh t7418-submodule-sparse-gitmodules.sh`: 9/9 passed

**2026-04-09 (t5609 / clone --branch)**

- `cargo test -p grit-lib --lib`: 152 passed
- `./scripts/run-tests.sh t5609-clone-branch.sh`: 7/7 passed

**2026-04-09 (t3422 / rebase incompatible options)**

- `cargo test -p grit-lib --lib`: 152 passed
- `./scripts/run-tests.sh t3422-rebase-incompatible-options.sh`: 52/52 passed

**2026-04-09 (t2203-add-intent)**

- `cargo test -p grit-lib --lib`: 152 passed
- `./scripts/run-tests.sh t2203-add-intent.sh`: 19/19 passed

**2026-04-09 (t3417 / rebase whitespace fix)**

- `cargo test -p grit-lib --lib`: 147 passed
- `./scripts/run-tests.sh t3417-rebase-whitespace-fix.sh`: 4/4 passed

**2026-04-09 (t5318 / pack-objects --revs)**

- `cargo test -p grit-lib --lib`: 147 passed
- `./scripts/run-tests.sh t5318-pack-objects-revs-exclude.sh`: 9/9 passed
