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
