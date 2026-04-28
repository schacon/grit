# Remote Auth Credential Model

## Scope

Claimed Phase 1 in `AUTH_TASKS.md`: replace the flat `BTreeMap` credential handling in `grit credential` with an ordered credential record that can preserve Git credential protocol fields.

## Changes

- Added an ordered `Credential` model in `grit/src/commands/credential.rs`.
- Preserved repeated `key[]` fields such as `capability[]`, `wwwauth[]`, and `state[]`.
- Implemented array reset semantics for empty `key[]=` entries.
- Kept scalar fields ordered in Git-compatible output order for helper interactions.
- Normalized `url=` into protocol, host, path, username, and password without losing the original context.
- Removed HTTP(S) path before helper invocation unless `credential.useHttpPath` is enabled.
- Added helper response merging that can represent pre-encoded `authtype` + `credential` responses.
- Added expiry filtering for `password_expiry_utc`.
- Added helper-chain stopping once a complete username/password or pre-encoded credential is available.
- Added `GIT_ASKPASS`, `core.askPass`, and `SSH_ASKPASS` lookup for missing credential prompts.
- Added `credential.protectProtocol` checks for decoded carriage returns in protocol/host values.
- Added encoded-newline URL rejection before helpers are invoked.
- Added prompt sanitization for unsafe credential prompt components, including control characters and spaces.
- Forwarded askpass stderr so upstream-style askpass prompt tests can observe prompt text.
- Added `grit credential capability` output for `authtype` and `state` support.
- Added URL-scoped credential config lookup for `username`, `useHttpPath`, `protectProtocol`, and `sanitizePrompt`.
- Adjusted credential parse/helper abort failures to use Git-shaped `fatal:` output.
- Added terminal prompting via `/dev/tty` when no askpass program is configured and interactive prompting is allowed.
- Updated built-in `credential-store` to avoid persisting credentials marked `ephemeral`.

## Validation

- `cargo check -p grit-rs`: passed.
- `cargo build --release -p grit-rs`: passed.
- `cargo test -p grit-lib --lib`: 197 passed.
- `./scripts/run-tests.sh t0300-credentials.sh`: skipped because `t0300-credentials` is currently `in_scope=skip` in `data/test-files.csv`.
- Manual smoke checks with `target/release/grit credential fill` verified:
  - Basic username/password helper output.
  - Capability-aware `authtype` + `credential` helper output.
  - Capability filtering when the caller did not advertise `authtype` support.
  - Encoded newline URL rejection.
  - `credential.protectProtocol` CR rejection and `credential.protectProtocol=false` override.
  - Sanitized askpass prompt for a control-character username.
  - `grit credential capability` output.
  - URL-scoped `credential.username`.
  - URL-scoped `credential.useHttpPath` and default HTTP path stripping.
  - Fatal output shape for missing protocol, encoded-newline URL rejection, and helper `quit`.
  - Built-in `credential-store` skips ephemeral credentials.

## Remaining Work

- Continue Phase 2 validation by enabling or directly running the upstream-derived credential harness when scope allows it.
- Enable or directly run `t0300-credentials.sh` once the harness scope allows it.

## Credential Store Parity

- Implemented Git-compatible lookup order: `~/.git-credentials`, then `$XDG_CONFIG_HOME/git/credentials` or `$HOME/.config/git/credentials`.
- Implemented write target selection: first existing default file, or create `~/.git-credentials` if none exist.
- Implemented erase across all relevant store files.
- Implemented overwrite-on-store by removing matching existing entries before appending the new credential.
- Implemented stricter stored URL parsing so invalid entries are ignored.
- Implemented protocol, host, username, and relevant-path matching.
- Preserved CRLF behavior where a CR belongs to the path when a stored URL has a path, but invalidates a host-only stored URL.
- Kept unreadable store files as non-fatal misses so later files can satisfy lookup.
- Verified `--file <path>` and `--file=<path>` behavior manually.
- Attempted `./scripts/run-tests.sh t0302-credential-store.sh`; it remains skipped by current harness scope, so no harness tests executed.

## Credential Cache Daemon

- Replaced the credential-cache stub with a Unix-socket daemon path.
- Implemented default socket paths:
  - `$XDG_CACHE_HOME/git/credential/socket` when `XDG_CACHE_HOME` is set.
  - `$HOME/.cache/git/credential/socket` by default.
  - `$HOME/.git-credential-cache/socket` when that directory exists.
- Implemented absolute `--socket` support.
- Implemented `store`, `get`, `erase`, and `exit`.
- Implemented timeout expiration and `password_expiry_utc` checks.
- Preserved confidential fields such as `oauth_refresh_token` in cached credential records.
- Ensured socket parent directories are created with restrictive permissions on Unix.
- Verified default socket creation, custom socket creation, store/get, erase, timeout expiry, and exit cleanup manually.

## SSH Command Parity

- Added `core.sshCommand` support for shell-based SSH invocation.
- Preserved precedence: `GIT_SSH_COMMAND`, then `core.sshCommand`, then `GIT_SSH`, then default `ssh`.
- Made `build_git_ssh_argv` default to `ssh` when `GIT_SSH` is unset, matching live spawn behavior.
- Kept `GIT_SSH_VARIANT` overriding `ssh.variant`.
- Extended submodule push remote URL classification to treat scp-style and `git+ssh://` URLs as SSH, not local paths.
- Validation: `t5507-remote-environment` passed 5/5; `t5813-proto-disable-ssh` remains 63/81 with known existing failures.

## Live SSH Upload-Pack

- Added `spawn_git_ssh_upload_pack` on top of the shared SSH service spawner.
- Wired `git ls-remote` for SSH URLs to run `git-upload-pack` over an external SSH subprocess.
- Reused the existing upload-pack advertisement and protocol-v2 `ls-refs` parsing path.
- Preserved `--upload-pack` custom remote command behavior for SSH `ls-remote`.
- Validation: `t5512-ls-remote` remains 16/40 and `t5601-clone` remains 64/115, with broader pre-existing failures still to address.
- Added live SSH fetch over `git-upload-pack` for unresolved SSH URLs.
- Reused existing upload-pack negotiation for v0/v1 and v2 fetch over SSH streams.
- Preserved the local SSH shortcut for test harness URLs that resolve to local repositories.
- Validation: `t5510-fetch` remains 199/215 and `t5700-protocol-v1` returned 0/0 warning from harness selection/status.
- Added live SSH clone over external `git-upload-pack` for unresolved SSH URLs.
- The live SSH clone path initializes the destination repository, fetches over SSH, writes remote-tracking refs, configures origin, and checks out the selected/default branch when possible.
- Validation: `t5601-clone` remains 64/115 and `t5603-clone-dirname` remains 25/47 with broader existing failures.

## SSH Push Hardening

- Allowed `--receive-pack` to flow through SSH push instead of rejecting it before transport selection.
- Kept `--receive-pack` rejected for HTTP and local native push paths where it is still unsupported.
- Passed custom receive-pack command names into the SSH service spawner.
- Validation: `t5406-remote-rejects` passed 3/3; `t5545-push-options` remains 2/13 with broader existing failures.

## Trace Redaction Audit

- Added shared HTTP URL credential scrubbing for display paths.
- Scrubbed URL username/password fields from HTTP access errors.
- Scrubbed URL username/password fields from ureq connection-level error contexts.
- Scrubbed URL username/password fields from curl request-start traces when trace redaction is enabled.
- Scrubbed URL username/password fields from trace2 `git-remote-https` child-start events.
- Confirmed existing curl trace redaction already covers `Authorization`, `Proxy-Authorization`, cookie values, and auth-like extra headers by default.
- Validation: `cargo fmt`, `cargo check -p grit-rs`, and `cargo build --release -p grit-rs` passed; manual HTTP trace/error smoke with URL userinfo passed.

## HTTP Proxy and Smart Regression Sweep

- Accepted Git transport's internal `pack-objects --all-progress-implied` flag.
- Implemented `unpack-objects --pack_header=<version>,<count>` by reconstructing the consumed PACK header before unpacking the remaining stream.
- Fixed `test-httpd` smart backend lookup for colon-separated `GIT_EXEC_PATH` and Homebrew/system backend locations.
- Added `upload-pack --http-backend-info-refs` and `--stateless-rpc` compatibility.
- Fixed stateless upload-pack responses so HTTP v2 POST responses are not prefixed with capability advertisements.
- Kept v0/v1 stateless HTTP POSTs from sending `Git-Protocol: version=2` after a v0 discovery response.
- Cached proxy askpass results in-process so a clone prompts once for a proxy URL even when it constructs multiple HTTP contexts.
- Cleared inherited `GIT_PROTOCOL` from `test-httpd` CGI children unless the request explicitly supplies a `Git-Protocol` header.
- Validation: `t5564-http-proxy` now reports 7/8 instead of timing out; `t5555-http-smart-common` passes 10/10; `t5581-http-curl-verbose` reports 1/2 with a current harness `git-remote-http` lookup issue; `t5549`, `t5539`, and `t5542` still show broader HTTP smart/shallow failures.
- Fixed SOCKS-over-Unix direct HTTP request construction so inserted headers are CRLF-delimited instead of concatenated onto the previous header.
- This restored parseable `Git-Protocol` and `Content-Length` headers for SOCKS smart HTTP discovery and stateless POSTs.
- Validation: `t5564-http-proxy` now passes 8/8; `t5555-http-smart-common` remains 10/10; `t5581-http-curl-verbose` remains 1/2 with the known harness `git-remote-http` lookup issue.
- Routed `error_git_upload_pack` HTTP clone through Grit in the lightweight HTTP harness so `t5581` exercises Grit's `GIT_CURL_VERBOSE` output.
- Added the `test-httpd` `500 Intentional Breakage` route for `/error_git_upload_pack/smart/...git-upload-pack`.
- Validation: `t5581-http-curl-verbose` now passes 2/2 while `t5564-http-proxy` remains 8/8 and `t5555-http-smart-common` remains 10/10.
- Routed HTTP push through Grit in the lightweight HTTP harness.
- Fixed server-side helper discovery by computing the real Git exec path with `GIT_EXEC_PATH` unset and adding a `git-receive-pack` wrapper beside the existing upload-pack wrapper.
- Allowed HTTP push source refspecs to resolve tags and general revisions via the shared push source resolver.
- Filtered remote-only advertised haves out of local `pack-objects --not` input so push packs can be built when the remote has unrelated objects.
- Emitted `write_pack_file/wrote` trace2 data for HTTP push packs from the generated pack header object count.
- Added initial `push.negotiate` handling: protocol-v2 pushes use local parent commits as common bases for thin-pack object counts, while non-v2 negotiation emits Git-compatible warnings and proceeds.
- Validation: `t5549-fetch-push-http` now passes 3/3; `t5555-http-smart-common`, `t5564-http-proxy`, and `t5581-http-curl-verbose` remain fully passing.
- Ignored `shallow <oid>` lines while parsing HTTP receive-pack advertisements.
- Stripped leading `+` before resolving HTTP push source refspecs.
- Decoded gzip request bodies in the lightweight HTTP server before passing them to `git-http-backend`, which fixes large receive-pack POSTs.
- Validation: `t5542-push-http-shallow` now passes 3/3; `t5549`, `t5555`, `t5564`, and `t5581` remain fully passing.
- Added lightweight `/custom_auth/` handling to `test-httpd` for `t5563-simple-http-auth` semantics.
- Routed `custom_auth` HTTP operations through Grit in the hybrid HTTP test wrapper.
- Preserved duplicate ureq response header values without multiplying them, so ordered `WWW-Authenticate` challenges reach credential helpers correctly.
- Approved proactively supplied credentials on first-request success, matching helper `store` expectations.
- Folded custom auth continuation response lines in the test HTTP server before sending headers.
- Parsed `status=... response=...` custom auth challenge lines so multistage auth gets the next challenge after an intermediate 401.
- Enabled `CGIPASSAUTH` when `lib-httpd.sh` explicitly sets the prereq and moved `t5563-simple-http-auth` into scope.
- Validation: `./scripts/run-tests.sh --timeout 120 t5563-simple-http-auth.sh` passes 17/17. Nearby official HTTP checks `t5555`, `t5564`, `t5581`, `t5549`, and `t5542` remain fully passing.
- Scoped the lightweight HTTP server to use Grit upload-pack for protocol-v2 bundle-uri HTTP tests.
- Added HTTP fetch support for sending and draining the protocol-v2 `bundle-uri` command when advertised and enabled, while suppressing it for explicit `--bundle-uri` clones.
- Reused the active HTTP client context for post-fetch bundle-uri lookups so auth/proxy/cookie state is not reset between fetch and bundle-uri discovery.
- Added packet tracing for HTTP protocol-v2 capability advertisements so clone/ls-remote assertions see `version 2` and `bundle-uri`.
- Validation: `t5732-protocol-v2-bundle-uri-http` now passes 9/9; nearby official HTTP checks `t5555`, `t5563`, `t5564`, and `t5581` remain fully passing.
- Routed authenticated smart HTTP paths through Grit in the lightweight HTTP harness.
- Fixed curl trace output for `GIT_TRACE_REDACT=0` to include the actual `Authorization` header value instead of a placeholder.
- Made the lightweight HTTP access log include an Apache-like byte-count field so `strip_access_log` preserves status codes.
- Validation: focused `t5551-http-fetch-smart` auth/redaction cases pass through Grit; official `t5551` is now in-scope at 29/37 with remaining real failures in empty SHA-256 clone object-format support, not auth.
- Accepted repeated `-v` for `git push`.
- Expanded default `push.default=matching` for HTTP remotes instead of bailing in the default-refspec path.
- Emitted Git-style `POST git-receive-pack` summaries for verbose HTTP pushes, including chunked requests.
- Sent a valid empty pack for HTTP receive-pack updates when the remote already has all pushed objects, fixing URL-scrub porcelain pushes.
- Reported client-side atomic pre-rejections with Git-like per-ref status lines, including collateral `atomic push failed` refs.
- Validation: `t5541-http-push-smart` now passes 21/21; `t5549-fetch-push-http` and `t5542-push-http-shallow` remain fully passing.

## HTTP Challenge Plumbing

- Added header retention to raw HTTP responses.
- Extracted `WWW-Authenticate` challenge values from 401 responses.
- Added folded header continuation handling for manually parsed HTTP responses.
- Passed `capability[]=authtype`, `capability[]=state`, and ordered `wwwauth[]` attributes to `grit credential fill`.
- Passed `wwwauth[]` attributes to `credential reject` so rejected credentials keep challenge context.
- Kept Basic credential approval requests free of challenge-only fields, matching Git's simple Basic auth expectations.
- Replaced Basic-only HTTP credential state with a typed auth representation.
- Added support for helper-provided pre-encoded credentials via `authtype` + `credential`, producing `Authorization: <authtype> <credential>`.
- Preserved existing Basic `Authorization` generation for username/password helper responses and askpass fallback.
- Added one-step multistage auth handling for helper responses with `continue=1`, carrying helper `state[]` and updated challenges into a second `credential fill`.
- Included pre-encoded auth fields and helper state in approve/reject credential input.
- Preserved ephemeral markers so helpers can avoid storing short-lived credentials.
- Added `http.proactiveAuth` parsing for `basic`, `auto`, and `none`.
- Added proactive Basic credential lookup before the first HTTP request.
- Added proactive auto credential lookup that can use helper-selected pre-encoded auth schemes.
- Parsed `http.emptyAuth`; for now it disables proactive auth and leaves the normal 401 path intact.
- Added global and URL-scoped `http.extraHeader` support with load-order reset semantics.
- Applied extra headers to ureq, HTTP proxy, and SOCKS-over-Unix request paths.
- Redacted authorization-like extra headers in `GIT_TRACE_CURL` output by default.
- Replaced the static cookie header with parsed cookie records matched per request URL.
- Added Netscape cookie file parsing with domain, path, secure, and expiration handling.
- Preserved simplified cookie/header-line parsing for existing `http.cookieFile` behavior.
- Added `http.saveCookies` support by appending received `Set-Cookie` headers to the configured cookie file.
- Routed raw HTTP bundle downloads in `bundle_uri.rs` through `HttpClientContext`.
- Routed protocol-v2 HTTP bundle-uri discovery in `bundle_uri.rs` and `http_bundle_uri.rs` through `HttpClientContext`.
- Added `http.sslVerify=false` and `GIT_SSL_NO_VERIFY` handling by configuring rustls with a permissive verifier when explicitly requested.
- Audited TLS options: CA file/path and client certificate/key options are not cleanly supported by the current ureq/rustls setup and remain documented limitations.
- Added environment proxy support for `http_proxy`, `https_proxy`, `all_proxy`, and `no_proxy`.
- Preserved `http.proxy` precedence over environment proxy variables.
- Added `http.proxyAuthMethod` and `GIT_HTTP_PROXY_AUTHMETHOD` parsing.
- Kept Basic/anyauth proxy credentials supported and fail clearly for unsupported proxy auth methods.
- Added `remote.<name>.proxy` override support for HTTP fetch and push before falling back to `http.proxy` or environment proxy variables.
