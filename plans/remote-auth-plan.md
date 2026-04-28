# Remote Authentication Implementation Plan

## Scope

This report analyzes authentication paths for Grit network operations, with emphasis on HTTP(S) and SSH remotes. It covers what is implemented today, what is incomplete relative to Git behavior, and a recommended order for finishing the work.

The main code paths reviewed are:

- HTTP transport and credentials: `grit/src/http_client.rs`, `grit/src/http_smart.rs`, `grit/src/http_push_smart.rs`
- Remote commands: `grit/src/commands/clone.rs`, `grit/src/commands/fetch.rs`, `grit/src/commands/push.rs`, `grit/src/commands/ls_remote.rs`
- Credential commands/helpers: `grit/src/commands/credential.rs`, `grit/src/commands/credential_store.rs`, `grit/src/commands/credential_cache.rs`
- SSH transport: `grit/src/ssh_transport.rs`
- Test tracking: `data/test-files.csv`, `plan.md`, and upstream tests in `git/t/`

## Current Overall State

Grit has a meaningful start on network authentication, but it is uneven:

- HTTP(S) fetch, clone, ls-remote, and push use a native smart-HTTP path. They can retry after a `401` with a Basic `Authorization` header built from credential helper output, URL username, `credential.username`, or `GIT_ASKPASS`.
- The credential command can call configured helpers, including built-in `store` and `cache` helper names. It handles URL-scoped helper config in load order and resets helper lists on empty values.
- SSH URL parsing and SSH command construction are fairly advanced. Grit understands scp-style and `ssh://`/`git+ssh://` URLs, ports, variants, `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`, `ssh.variant`, IPv4/IPv6 flags, and `GIT_PROTOCOL`.
- SSH authentication itself is intentionally delegated to the external SSH program. Grit does not implement key loading, ssh-agent protocol, password prompts, host key checking, or SSH config parsing internally.
- Local test-harness SSH remotes are often short-circuited to local repositories. That passes many URL/protocol tests but is not the same as full live SSH fetch support.

The biggest gap is not "no auth exists"; it is that HTTP only supports a narrow Basic-style flow, credential protocol support is incomplete, and SSH fetch/clone still leans on local resolution where real Git would stream upload-pack over SSH.

## HTTP(S) Authentication

### Implemented

`HttpClientContext` is the shared HTTP client for smart HTTP. It supports:

- `http://` and `https://` URLs through `ureq`.
- Smart HTTP request headers, including `User-Agent`, `Git-Protocol`, content type, accept headers, gzip request bodies, and chunked POSTs.
- A retry flow on HTTP `401` for GET and POST.
- Basic `Authorization` headers using `username:password` encoded with Base64.
- Credential helper fill/approve/reject around successful or failed authentication.
- `credential.useHttpPath` for deciding whether the path is sent to helpers.
- `credential.username` and URL usernames as default usernames.
- `GIT_ASKPASS` prompts for missing username or password.
- In-process auth caching after successful auth, reused by the same `HttpClientContext`.
- `http.cookieFile` input, emitted as a `Cookie` header.
- `http.proxy`, including:
  - direct `ureq` proxy support for non-HTTP proxy forms,
  - hand-built HTTP forward proxy requests,
  - Basic `Proxy-Authorization` from proxy URL userinfo,
  - SOCKS over Unix socket handling for the existing proxy tests,
  - `GIT_ASKPASS` for a proxy URL that has a username but no password.
- Trace output for `GIT_TRACE_CURL`, `GIT_TRACE_CURL_COMPONENTS`, and redaction via `GIT_TRACE_REDACT`.

The native smart-HTTP stack is connected to:

- `clone` via `run_http_clone`.
- `fetch` via `http_fetch_pack`.
- `fetch --negotiate-only` via `http_negotiate_only_common`.
- `ls-remote` via `http_ls_refs`.
- `push` via `discover_receive_pack` and `send_receive_pack`.

### Partially Implemented or Missing

Credential protocol support is incomplete:

- `grit credential` does not implement the `capability` action.
- Multi-valued keys such as `capability[]`, `wwwauth[]`, and `state[]` are parsed into a `BTreeMap<String, String>`, so repeated values are lost.
- Helper chaining currently runs all helpers for `fill`; Git stops once it has a complete credential or a usable pre-encoded credential.
- `quit`, `ephemeral`, `continue`, `authtype`, `credential`, `password_expiry_utc`, and `oauth_refresh_token` semantics are not fully modeled.
- HTTP callers do not pass `capability[]=authtype`, `capability[]=state`, or `wwwauth[]` challenges to helpers.
- HTTP callers only build Basic auth from `username` and `password`; they do not use helper-provided `authtype` plus `credential`, so Bearer, Digest, NTLM-style multistage, and OAuth helper flows are not supported.
- `credential.interactive`, `credential.sanitizePrompt`, and `credential.protectProtocol` are not enforced.
- Prompt fallback is only `GIT_ASKPASS`; Git also supports `core.askPass`, `SSH_ASKPASS`, then terminal prompting.

HTTP authentication itself is incomplete:

- `http.proactiveAuth` and `http.emptyAuth` are not implemented.
- `WWW-Authenticate` parsing is not implemented, including multiple headers, mixed case names, and folded continuations.
- `http.proxyAuthMethod` / `GIT_HTTP_PROXY_AUTHMETHOD` are not implemented. Proxy auth is effectively Basic-from-URL only.
- HTTPS certificate/client-auth knobs from Git are not implemented: `http.sslVerify`, `GIT_SSL_NO_VERIFY`, `http.sslCert`, `http.sslKey`, CA path/info, SSL version, ciphers, backend, and proxy SSL certificate options.
- `http.extraHeader` is not implemented.
- `http.saveCookies` is not implemented; `http.cookieFile` is read-only and only handles a simplified cookie/header form.
- Environment proxy variables (`http_proxy`, `https_proxy`, `all_proxy`, `no_proxy`) are not explicitly handled in the Git-compatible way.
- URL credential redaction exists in push display, but credentials embedded in URLs should be audited across all errors, traces, and helper inputs.

Credential helper implementations are incomplete:

- `credential-store` only uses `~/.git-credentials` by default. Git also searches and writes according to `$XDG_CONFIG_HOME/git/credentials` precedence.
- `credential-store` matching is too loose; it does not fully match protocol, host, username, path/useHttpPath, invalid lines, CRLF rules, duplicate replacement, or erase across all configured files.
- `credential-cache` is a stub. It consumes input and reports misses; it has no Unix socket daemon, no timeout, no storage, and no `exit` control path.

### Relevant Tests

Most directly relevant:

- `t0300-credentials`: skipped in `data/test-files.csv` and listed in `plan.md` as `3/56`.
- `t0301-credential-cache`: skipped in `data/test-files.csv` and listed as `6/52`.
- `t0302-credential-store`: skipped in `data/test-files.csv` and listed as `5/65`.
- `t0303-credential-external`: skipped in `data/test-files.csv`, but recorded as `23/23`.
- `t5563-simple-http-auth`: skipped in `data/test-files.csv`; upstream covers Basic, Bearer, proactive auth, helper capabilities, `WWW-Authenticate`, invalid credentials, and multistage auth.
- `t5564-http-proxy`: in scope, partially passing (`3/8` in `plan.md`; `0/0 timeout` in `data/test-files.csv` suggests the tracking row needs refreshing).
- `t5581-http-curl-verbose`: passing and relevant for trace redaction/HTTP error shape.
- `t5541-http-push-smart`, `t5551-http-fetch-smart`, `t5539-fetch-http-shallow`, `t5542-push-http-shallow`: broader HTTP transport correctness tests that auth changes should not regress.

## SSH Authentication

### Implemented

`grit/src/ssh_transport.rs` implements Git-like SSH URL parsing and command construction:

- scp-style `host:path` URLs.
- `ssh://` and `git+ssh://` URLs.
- user-host forms, bracketed IPv6, and numeric ports.
- checks that host/path do not begin with `-`.
- `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`, and `ssh.variant`.
- variant-specific port flags (`-p` for OpenSSH, `-P` for Plink/Putty).
- `-4` / `-6` handling.
- OpenSSH `SendEnv=GIT_PROTOCOL` for protocol negotiation.
- `GIT_PROTOCOL` propagation to the child where appropriate.
- remote command quoting for `git-upload-pack` and `git-receive-pack`.
- test-harness recording for `test-fake-ssh`.

Push has a true streaming SSH path for unresolved remote repositories:

- `push_to_ssh_url` spawns `git-receive-pack` over SSH.
- It reads the receive-pack advertisement from SSH stdout.
- It sends update commands and pack data over the child stdin/stdout streams.

Clone/fetch have partial SSH behavior:

- If the SSH URL resolves to a local repo via `try_local_git_dir`, Grit performs local object transfer and records expected fake-SSH output for tests.
- If an SSH clone cannot resolve locally, Grit invokes the configured SSH command as a probe and then errors.
- Fetch of an unresolved SSH URL errors instead of opening a live SSH upload-pack stream.

### Partially Implemented or Missing

SSH authentication is delegated to the external SSH executable, which is the right high-level model for Git compatibility. The missing work is in complete SSH transport integration, not in writing an SSH authentication stack:

- Live SSH fetch is not implemented. `fetch` requires the SSH URL to resolve to a local repository and otherwise errors.
- Live SSH clone does not consume upload-pack output; it invokes the SSH command and then errors if local resolution failed.
- `core.sshCommand` is not handled in `ssh_transport.rs`, even though Git treats it with the same precedence family as `GIT_SSH_COMMAND`.
- Precedence and exact behavior between `GIT_SSH_COMMAND`, `GIT_SSH`, `core.sshCommand`, `ssh.variant`, and `GIT_SSH_VARIANT` should be audited against Git.
- No internal handling exists for SSH keys, ssh-agent, password auth, keyboard-interactive prompts, `known_hosts`, host key verification, or `~/.ssh/config`. This is acceptable if Grit delegates to OpenSSH/plink, but it should be documented as an intentional boundary.
- There is no dedicated SSH askpass handling in Grit. For delegated SSH, askpass is handled by the SSH program/environment, not by Grit.
- Remote command names are mostly upload-pack/receive-pack specific; more edge behavior around custom paths and `--upload-pack` / `--receive-pack` should be validated with upstream tests.
- Push over live SSH exists but should be hardened after fetch/clone stream support exists, because it shares receive-pack protocol code with HTTP and may still miss edge cases around protocol v2, sideband, push options, and error propagation.

### Relevant Tests

Most directly relevant:

- `t5813-proto-disable-ssh`: `plan.md` lists this as complete, while `data/test-files.csv` currently records `63/81`; treat it as important regression-check coverage for protocol policy and URL detection.
- `t5602-clone-remote-exec`: passing; covers remote exec command behavior.
- `t5601-clone`, `t5603-clone-dirname`, `t5606-clone-options`, `t5611-clone-config`: SSH-adjacent clone behavior and fake-SSH argv compatibility.
- `t5510-fetch`, `t5512-ls-remote`, `t5700-protocol-v1`: useful for upload-pack behavior over stream transports.
- `t5547-push-quarantine`, `t5548-push-porcelain`, `t5545-push-options`: useful after live SSH push is hardened.
- `t7031-verify-tag-signed-ssh` and `t7528-signed-commit-ssh` are SSH-signature tests, not remote SSH authentication. They should not drive network auth implementation.

## Other Network Paths

`git://` transport is implemented for fetch/clone/ls-remote with native socket negotiation, but it has no authentication by design. Push over `git://` is explicitly unsupported.

`ext::` remote helper style and local/file transports can execute commands, but they are not authentication mechanisms. They matter because they share protocol, helper, and security boundaries with remote operations.

## Recommended Implementation Order

### 1. Finish Credential Plumbing First

This should come before more HTTP auth work, because HTTP auth will keep becoming ad hoc until the credential model can represent Git's full credential protocol.

Target behavior:

- Add a typed credential data model that preserves repeated attributes in order.
- Implement `git credential capability`.
- Implement helper chaining stop conditions: stop on full username/password, stop on `authtype` + `credential`, honor `quit`.
- Preserve and round-trip `capability[]`, `wwwauth[]`, `state[]`, `authtype`, `credential`, `ephemeral`, `continue`, `password_expiry_utc`, and `oauth_refresh_token`.
- Implement expiry checks and avoid storing ephemeral credentials where Git would avoid it.
- Add prompt fallback order: `GIT_ASKPASS`, `core.askPass`, `SSH_ASKPASS`, terminal.
- Enforce `credential.interactive=false` and prompt sanitization/protocol protection.

Tests to drive this phase:

- Start with `t0300-credentials`.
- Then use `t0303-credential-external` to ensure external helper compatibility remains intact.

### 2. Make `credential-store` Git-Compatible

This is a bounded helper and makes many credential tests concrete without needing sockets.

Target behavior:

- Implement the default search/write order for `~/.git-credentials` and `$XDG_CONFIG_HOME/git/credentials`.
- Match credentials by protocol, host, username, and path according to Git's rules.
- Respect `credential.useHttpPath`.
- Handle CRLF, invalid lines, comments/empty lines, duplicate entries, erase from all files, and file permissions.
- Support `--file=<path>` as well as `--file <path>` if the parser does not already.

Tests to drive this phase:

- `t0302-credential-store`.
- Re-run relevant `t0300` helper-chain tests after changes.

### 3. Implement Real `credential-cache`

This is more infrastructure than HTTP-specific auth, but it is part of Git's supported credential story.

Target behavior:

- Implement a Unix socket daemon or daemon-equivalent process.
- Default socket path selection: `$XDG_CACHE_HOME/git/credential/socket` unless `~/.git-credential-cache/` exists.
- `--socket`, `--timeout`, `get`, `store`, `erase`, and `exit`.
- Store expiration by timeout and password expiry.
- Enforce socket directory permissions enough to satisfy Git's security expectations.

Tests to drive this phase:

- `t0301-credential-cache`.

### 4. Upgrade HTTP Auth From Basic-Only to Git Credential Auth

Once credential objects support capabilities and repeated fields, wire that into `HttpClientContext`.

Target behavior:

- Parse `WWW-Authenticate` response headers, case-insensitively, including multiple headers and continuations.
- Send `capability[]=authtype` and `capability[]=state` to helpers.
- Pass `wwwauth[]` challenges to helpers.
- Accept helper-provided `authtype` + `credential` and build `Authorization: <authtype> <credential>`.
- Continue supporting Basic from username/password.
- Implement invalid-credential reject with the same fields Git passes to helpers.
- Implement multistage auth loops using `continue` and `state[]`.
- Implement `http.proactiveAuth` (`basic`, `auto`, `none`) and `http.emptyAuth`.
- Keep one auth decision available for all RPC probes/posts in a session without leaking stale rejected auth.

Tests to drive this phase:

- `t5563-simple-http-auth`.
- `t5581-http-curl-verbose` for trace behavior.
- Then broader smart HTTP tests: `t5551-http-fetch-smart`, `t5541-http-push-smart`, `t5539-fetch-http-shallow`, `t5542-push-http-shallow`.

### 5. Fill In HTTP Proxy, TLS, and Header Compatibility

This can follow basic HTTP auth because many of these are independent knobs, but they sit on the same request-construction boundary.

Target behavior:

- `http.proxyAuthMethod` and `GIT_HTTP_PROXY_AUTHMETHOD`.
- Proxy challenge handling if needed by tests.
- Environment proxy variables and no-proxy behavior.
- `http.extraHeader`, including multiple values and empty reset.
- HTTPS verification/certificate options that are reasonably possible with the chosen HTTP stack.
- `http.cookieFile` Netscape cookie format, matching by domain/path where needed.
- `http.saveCookies`.
- Audit credential redaction in trace/error output.

Tests to drive this phase:

- `t5564-http-proxy`.
- `t5581-http-curl-verbose`.
- Later, HTTPD and backend tests that use SSL/cookies/headers if brought in scope.

### 6. Implement Live SSH Fetch/Clone Streaming

Do not implement an SSH protocol or key agent in Rust. Keep delegating authentication to the configured SSH executable, like Git does. The missing piece is consuming the SSH stream for upload-pack.

Target behavior:

- Add a `spawn_git_ssh_upload_pack` sibling to `spawn_git_ssh_receive_pack`.
- Make `clone` use the upload-pack stream when an SSH URL does not resolve locally.
- Make `fetch` use the upload-pack stream when an SSH URL does not resolve locally.
- Reuse existing upload-pack negotiation code over bidirectional child streams.
- Preserve fake-SSH local resolution behavior for existing tests.
- Add `core.sshCommand` support and audit command precedence.
- Verify protocol v0/v1/v2, `GIT_PROTOCOL`, `--upload-pack`, IPv4/IPv6, port options, and error propagation.

Tests to drive this phase:

- Start with fake-SSH argv tests in `t5601-clone`, `t5602-clone-remote-exec`, and `t5603-clone-dirname`.
- Then run stream-oriented fetch/ls-remote tests such as `t5510-fetch`, `t5512-ls-remote`, and `t5700-protocol-v1`.
- Keep `t5813-proto-disable-ssh` passing throughout.

### 7. Harden SSH Push and Shared Stream Behavior

After live upload-pack is stable, harden receive-pack over SSH and shared protocol mechanics.

Target behavior:

- Validate `push_to_ssh_url` against sideband, push options, atomic push, porcelain output, and hook/error propagation.
- Confirm `--receive-pack` behavior matches Git.
- Ensure status reporting does not expose credentials embedded in SSH URLs.
- Keep the boundary clear: external SSH handles user authentication, host key verification, and SSH config.

Tests to drive this phase:

- `t5545-push-options`.
- `t5547-push-quarantine`.
- `t5548-push-porcelain`.
- Existing local push tests that already exercise receive-pack status parsing.

## Suggested Milestones

1. `t0300-credentials` mostly passing with a typed credential model.
2. `t0302-credential-store` fully passing.
3. `t0301-credential-cache` fully passing or explicitly scoped if daemon work is deferred.
4. `t5563-simple-http-auth` enabled and passing for Basic, Bearer, proactive, invalid, and multistage auth.
5. `t5564-http-proxy` fully passing with proxy auth behavior clarified.
6. Live SSH clone/fetch works through external SSH for real remote streams while existing fake-SSH tests still pass.
7. SSH push stream tests are expanded and hardened.

## Risks and Design Notes

- `ureq` may not expose every TLS, proxy, auth-challenge, or cookie behavior Git gets from libcurl. If compatibility pressure grows, consider a thin internal HTTP transport abstraction before adding more request-special cases.
- Credential protocol data should become typed before adding more auth behavior. Continuing to store credentials in a flat `BTreeMap<String, String>` will make `wwwauth[]`, `state[]`, and helper capabilities fragile.
- SSH should remain delegated to the user's SSH command. Implementing keys, agents, or host key verification in Grit would be large, security-sensitive, and less Git-compatible than using OpenSSH/plink.
- Many HTTP tests are currently skipped or have stale-looking tracking counts. After each auth milestone, refresh `data/test-files.csv` via the harness so the dashboard reflects reality.
