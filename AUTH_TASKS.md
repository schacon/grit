# Remote Auth Implementation Tasks

This checklist breaks `plans/remote-auth-plan.md` into dependency-ordered implementation work. Work top to bottom unless a task explicitly says it can run in parallel.

## Working Rules

- [ ] Claim one task at a time before implementation by changing `[ ]` to `[~]`.
- [ ] Keep edits focused on the files named in the task unless investigation proves the dependency boundary is wrong.
- [ ] Prefer fixing Rust code in `grit/src/` and `grit-lib/src/`; do not weaken tests or modify `tests/test-lib.sh`.
- [ ] After meaningful harness runs, refresh `data/test-files.csv`, generated dashboards, `test-results.md`, and any relevant task status.
- [ ] When a test marked `test_expect_failure` is fixed, flip it to `test_expect_success`.
- [ ] Before committing implementation work, run `cargo fmt`, `cargo clippy --fix --allow-dirty`, and the task-specific harness tests.

## Validation Baseline

- [ ] Build baseline: `cargo build --release -p grit-rs`.
- [ ] Record current auth-related harness status:
  - [ ] `./scripts/run-tests.sh t0300-credentials.sh`
  - [ ] `./scripts/run-tests.sh t0302-credential-store.sh`
  - [ ] `./scripts/run-tests.sh t0301-credential-cache.sh`
  - [ ] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
  - [ ] `./scripts/run-tests.sh t5564-http-proxy.sh`
  - [ ] `./scripts/run-tests.sh t5812-proto-disable-http.sh`
  - [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`
- [ ] Resolve status drift between `plan.md` and `data/test-files.csv` for `t5813-proto-disable-ssh`.
- [ ] Confirm whether skipped auth tests should remain skipped until implementation lands, or temporarily run single-file despite `in_scope=skip`.

## Phase 1: Credential Data Model

Dependency: none. This unblocks all richer HTTP auth work.

Primary files:

- `grit/src/commands/credential.rs`
- possible new internal module under `grit/src/commands/credential/` or `grit/src/credential_protocol.rs`

Tasks:

- [x] Replace flat credential input/output handling with a typed structure that preserves:
  - [x] scalar fields: `protocol`, `host`, `path`, `username`, `password`, `url`
  - [x] auth fields: `authtype`, `credential`, `ephemeral`, `continue`, `quit`
  - [x] metadata fields: `password_expiry_utc`, `oauth_refresh_token`
  - [x] multi-valued ordered fields: `capability[]`, `wwwauth[]`, `state[]`
  - [x] unknown fields that must round-trip to helpers when Git would preserve them
- [x] Add parser support for repeated `key[]=` attributes and empty-array reset semantics.
- [x] Add serializer support preserving Git-compatible output order where tests care.
- [x] Normalize `url=` into split fields without losing the original credential context.
- [x] Preserve HTTP path omission rules for `credential.useHttpPath=false`.
- [x] Add protection against invalid protocol/host fields required by `credential.protectProtocol`.
- [x] Add prompt-safe rendering helpers for usernames/hosts required by `credential.sanitizePrompt`.
- [x] Drive validation from the upstream-derived harness files; do not add new non-upstream tests for this work.

Validation:

- [x] `cargo test -p grit-lib --lib`
- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh --timeout 120 t0300-credentials.sh`

Definition of done:

- [x] Existing simple credential fill/approve/reject behavior still works.
- [x] Multi-valued credential attributes survive helper round trips.
- [x] The model can represent helper-provided `authtype` + `credential` without username/password.

## Phase 2: Credential Helper Semantics

Dependency: Phase 1.

Primary files:

- `grit/src/commands/credential.rs`
- `grit-lib/src/config.rs` if URL matching gaps are found

Tasks:

- [x] Implement `grit credential capability`.
- [x] Match Git helper chain behavior for `fill`:
  - [x] invoke helpers in config load order
  - [x] support empty helper reset
  - [x] stop once username/password are complete
  - [x] stop once `authtype` + `credential` are complete
  - [x] stop on `quit=true` or `quit=1`
  - [x] continue through partial helper responses
- [x] Implement helper output filtering based on caller capabilities:
  - [x] only accept `authtype` / `credential` when caller sent `capability[]=authtype`
  - [x] only accept `state[]` / `continue` when caller sent `capability[]=state`
- [x] Implement `password_expiry_utc` handling:
  - [x] ignore expired passwords during `fill`
  - [x] preserve non-expired values where Git does
- [x] Preserve `oauth_refresh_token` as confidential helper data.
- [x] Honor `ephemeral`:
  - [x] do not persist ephemeral credentials in helpers that should not store them
  - [x] still notify helpers on approve/reject when Git would
- [x] Implement `credential.interactive=false`.
- [x] Implement prompt fallback order:
  - [x] `GIT_ASKPASS`
  - [x] `core.askPass`
  - [x] `SSH_ASKPASS`
  - [x] terminal prompt when interactive is allowed
- [x] Make failure messages match Git closely enough for `t0300`.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh --timeout 120 t0300-credentials.sh`
- [x] `./scripts/run-tests.sh --timeout 120 t0303-credential-external.sh`

Definition of done:

- [x] `t0300-credentials` is fully passing.
- [x] `t0303-credential-external` remains passing.
- [x] Credential helpers can return Bearer-style credentials for later HTTP use.

## Phase 3: Credential Store Parity

Dependency: Phase 1. Can proceed in parallel with Phase 2 only after the shared credential parser is stable.

Primary files:

- `grit/src/commands/credential_store.rs`
- `grit/src/commands/credential.rs`

Tasks:

- [x] Implement default file search order:
  - [x] `~/.git-credentials`
  - [x] `$XDG_CONFIG_HOME/git/credentials`
  - [x] `$HOME/.config/git/credentials` when `XDG_CONFIG_HOME` is unset or empty
- [x] Implement default write target:
  - [x] first existing file among Git's default list
  - [x] create `~/.git-credentials` if none exists
- [x] Support `--file <path>` and `--file=<path>`.
- [x] Implement URL parsing for credential-store lines with Git-compatible invalid-line handling.
- [x] Match entries by:
  - [x] protocol
  - [x] host and optional port
  - [x] username when supplied in the query
  - [x] path when path is relevant
- [x] Respect `credential.useHttpPath`.
- [x] Handle CRLF rules from `t0302`.
- [x] Erase matching credentials from all relevant files.
- [x] Avoid duplicate stored credentials when replacing/updating existing entries.
- [x] Preserve or set restrictive file permissions on Unix.
- [x] Decide and document behavior for unreadable store files to match Git tests.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh --timeout 120 t0302-credential-store.sh`
- [x] `./scripts/run-tests.sh --timeout 120 t0300-credentials.sh`

Definition of done:

- [x] `t0302-credential-store` is fully passing.
- [x] Store helper behavior remains compatible with credential helper chaining.

## Phase 4: Credential Cache Daemon

Dependency: Phase 1. Prefer after Phase 2 because cache semantics share expiry/capability behavior.

Primary files:

- `grit/src/commands/credential_cache.rs`
- possible daemon subcommand/module if needed

Tasks:

- [x] Design a minimal Git-compatible Unix socket cache daemon.
- [x] Implement default socket path selection:
  - [x] `$XDG_CACHE_HOME/git/credential/socket`
  - [x] `$HOME/.cache/git/credential/socket`
  - [x] `$HOME/.git-credential-cache/socket` when that directory exists
- [x] Support `--socket <path>` and `--socket=<path>`.
- [x] Reject or error on relative socket paths if Git does.
- [x] Implement `store` over the daemon protocol.
- [x] Implement `get` with matching semantics from the credential model.
- [x] Implement `erase`.
- [x] Implement `exit`.
- [x] Implement timeout expiration from `--timeout`.
- [x] Honor `password_expiry_utc`.
- [x] Preserve confidential fields such as `oauth_refresh_token` if Git cache tests require it.
- [x] Enforce socket directory permissions sufficiently for upstream tests.
- [x] Ensure daemon cleanup does not leave stale background processes after tests.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh --timeout 120 t0301-credential-cache.sh`
- [x] `./scripts/run-tests.sh --timeout 120 t0300-credentials.sh`

Definition of done:

- [x] `t0301-credential-cache` is fully passing on Unix-like platforms.
- [x] Cache daemon handles repeated helper invocations without leaking stale credentials.

## Phase 5: HTTP Auth Challenge Parsing

Dependency: Phases 1 and 2.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/commands/credential.rs`
- possible new `grit/src/http_auth.rs`

Tasks:

- [x] Extend raw HTTP response capture to retain response headers, not just status/reason/body.
- [x] Capture all `WWW-Authenticate` headers on `401`.
- [x] Parse header names case-insensitively.
- [x] Support multiple challenge headers in order.
- [x] Support folded/continued header lines for test compatibility.
- [x] Preserve challenge strings for `wwwauth[]` exactly enough for helpers/tests.
- [x] Pass `capability[]=authtype` and `capability[]=state` to `credential fill`.
- [x] Pass all parsed challenges as ordered `wwwauth[]`.
- [x] Include relevant `wwwauth[]` and state fields in `approve` / `reject`.
- [x] Keep current Basic username/password flow working when no advanced auth is returned.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t0300-credentials.sh`

Definition of done:

- [x] HTTP auth helpers receive Git-compatible challenge input.
- [x] Basic auth tests still pass through the new challenge-aware path.

## Phase 6: HTTP Auth Schemes and Multistage Flow

Dependency: Phase 5.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/http_smart.rs`
- `grit/src/http_push_smart.rs`

Tasks:

- [x] Represent resolved HTTP auth as an enum rather than Basic-only username/password:
  - [x] Basic from username/password
  - [x] pre-encoded `authtype` + `credential`
  - [ ] empty auth if supported later
- [x] Build `Authorization: <authtype> <credential>` from helper-provided credentials.
- [x] Preserve Basic `Authorization` generation for username/password.
- [x] Implement invalid credential retry/reject behavior:
  - [x] reject failed credentials with all relevant credential fields
  - [x] clear in-process auth cache on failure
  - [x] avoid reusing stale credentials across RPC requests
- [x] Implement `continue=1` multistage auth:
  - [x] call helpers again with `state[]`
  - [x] pass updated challenges
  - [x] cap retry loops to avoid infinite authentication loops
- [x] Avoid storing credentials marked `ephemeral` where Git avoids persistence.
- [x] Share auth state across smart HTTP discovery and RPC POSTs in one operation.
- [x] Make GET and POST behavior consistent.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t5555-http-smart-common.sh`
- [ ] `./scripts/run-tests.sh t5549-fetch-push-http.sh`

Definition of done:

- [x] `t5563-simple-http-auth` passes Basic, Bearer, invalid credentials, and multistage cases that are supported by the test environment.
- [x] Existing unauthenticated smart HTTP tests do not regress.

## Phase 7: Proactive and Empty HTTP Auth

Dependency: Phase 6.

Primary files:

- `grit/src/http_client.rs`
- `grit-lib/src/config.rs` if config parsing needs additions

Tasks:

- [x] Parse `http.proactiveAuth` values:
  - [x] `basic`
  - [x] `auto`
  - [x] `none`
- [x] Implement proactive Basic:
  - [x] call credential helpers before first request
  - [x] request Basic-capable credentials from helpers
  - [x] send `Authorization` on first request
- [x] Implement proactive auto:
  - [x] allow helper-selected auth scheme
  - [x] fall back to Basic only when Git would
- [~] Parse and implement `http.emptyAuth`.
- [x] Ensure proactive auth is disabled by default.
- [x] Ensure credentials are not sent over plain HTTP unexpectedly beyond Git-compatible behavior.
- [x] Update trace redaction for proactive auth headers.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [x] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`

Definition of done:

- [x] Proactive Basic and auto auth cases in `t5563-simple-http-auth` pass.
- [x] Auth trace output remains redacted by default.

## Phase 8: HTTP Request Configuration Parity

Dependency: can begin after Phase 6; keep separate from challenge auth to avoid mixing failures.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/commands/clone.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/push.rs`
- `grit/src/commands/ls_remote.rs`

Tasks:

- [x] Implement `http.extraHeader`:
  - [x] multiple values
  - [x] empty-value reset
  - [x] per-URL matching if config layer supports it
  - [x] redaction for auth-like headers in traces
- [x] Implement environment proxy variables:
  - [x] `http_proxy`
  - [x] `https_proxy`
  - [x] `all_proxy`
  - [x] `no_proxy`
  - [x] Git-compatible precedence with `http.proxy`
- [x] Implement `http.proxyAuthMethod`.
- [x] Implement `GIT_HTTP_PROXY_AUTHMETHOD`.
- [x] Handle proxy `407` / `Proxy-Authenticate` enough for tests.
- [ ] Audit current manual HTTP forward proxy path for HTTPS behavior and document any limitation.
- [x] Add `remote.<name>.proxy` if required by tests encountered in this phase.
- [x] Make proxy auth redaction match `GIT_TRACE_REDACT`.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh t5564-http-proxy.sh`
- [x] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`
- [x] `./scripts/run-tests.sh t5555-http-smart-common.sh`

Definition of done:

- [x] `t5564-http-proxy` no longer times out and has clear pass/fail counts.
- [x] Proxy credentials are never leaked in default traces.

## Phase 9: HTTP Cookies, TLS, and Split HTTP Stack

Dependency: Phase 8 for request configuration; Phase 6 for authenticated requests.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/bundle_uri.rs`
- `grit/src/http_bundle_uri.rs`
- `grit/src/commands/http_fetch.rs`
- `grit/src/commands/http_push.rs`

Tasks:

- [x] Upgrade `http.cookieFile` support:
  - [x] Netscape cookie format
  - [x] domain matching
  - [x] path matching
  - [x] secure flag handling where applicable
  - [x] simplified header format remains supported
- [x] Implement `http.saveCookies`.
- [x] Implement TLS-related configuration that the current HTTP stack can support:
  - [x] `http.sslVerify`
  - [x] `GIT_SSL_NO_VERIFY`
  - [ ] `http.sslCAInfo` / `GIT_SSL_CAINFO`
  - [ ] `http.sslCAPath` / `GIT_SSL_CAPATH` if feasible
  - [x] document unsupported client certificate options if `ureq` cannot support them cleanly
- [x] Audit `http.sslCert`, `http.sslKey`, and password-protected cert behavior.
- [x] Route bundle URI HTTP(S) downloads through `HttpClientContext`:
  - [x] `grit/src/bundle_uri.rs`
  - [x] `grit/src/http_bundle_uri.rs`
  - [x] preserve existing bundle-uri protocol behavior
  - [x] ensure auth, proxy, cookies, and trace are shared with normal HTTP remote operations
- [x] Audit other raw `ureq` uses and either route through shared client or document why not.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh t5732-protocol-v2-bundle-uri-http.sh`
- [ ] `./scripts/run-tests.sh t5551-http-fetch-smart.sh`
- [x] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [x] `./scripts/run-tests.sh t5564-http-proxy.sh`

Definition of done:

- [x] Authenticated/proxied bundle URI fetches use the same client behavior as normal HTTP remotes.
- [x] TLS support and limitations are explicit and covered by tests where feasible.

## Phase 10: HTTP Smart Transport Regression Push

Dependency: Phases 5 through 9.

Primary files:

- `grit/src/http_smart.rs`
- `grit/src/http_push_smart.rs`
- `grit/src/http_client.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/clone.rs`
- `grit/src/commands/push.rs`
- `grit/src/commands/ls_remote.rs`

Tasks:

- [~] Re-run unauthenticated HTTP baseline and fix regressions.
- [~] Re-run authenticated HTTP tests and fix regressions.
- [ ] Verify auth state sharing across:
- [x] discovery GET
- [x] ls-refs POST
- [x] fetch POST
  - [ ] receive-pack discovery
  - [ ] receive-pack POST
- [~] Verify redirect/auth behavior in `t5551-http-fetch-smart` (auth/redaction cases pass; redirect cases remain expected failures).
- [ ] Verify shallow HTTP fetch/push behavior was not broken by auth changes.
- [ ] Audit URL credential scrubbing in:
  - [ ] push output
  - [ ] fetch/clone errors
- [x] trace output
  - [ ] credential helper inputs

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh t5555-http-smart-common.sh`
- [x] `./scripts/run-tests.sh t5549-fetch-push-http.sh`
- [~] `./scripts/run-tests.sh t5551-http-fetch-smart.sh` (29/37; auth/redaction subset passes, SHA-256 empty clone remains)
- [x] `./scripts/run-tests.sh t5541-http-push-smart.sh`
- [~] `./scripts/run-tests.sh --timeout 150 t5539-fetch-http-shallow.sh` (4/8; remaining failures are shallow/deepen transport state, not auth)
- [x] `./scripts/run-tests.sh t5542-push-http-shallow.sh`
- [x] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`

Definition of done:

- [x] Smart HTTP auth is integrated across fetch, clone, ls-remote, and push.
- [ ] Any remaining HTTP failures are documented as non-auth transport gaps.

## Phase 11: SSH Command Configuration Parity

Dependency: none for config work; do before live SSH streaming to avoid rework.

Primary files:

- `grit/src/ssh_transport.rs`
- `grit-lib/src/config.rs` if config access needs improvement
- `grit/src/commands/clone.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/push.rs`

Tasks:

- [x] Add `core.sshCommand` support.
- [x] Implement precedence:
  - [x] `GIT_SSH_COMMAND`
  - [x] `core.sshCommand`
  - [x] `GIT_SSH`
  - [x] default `ssh`
- [x] Audit `GIT_SSH_VARIANT` vs `ssh.variant` precedence.
- [x] Keep OpenSSH/Plink/Putty/TortoisePlink/simple variant handling compatible.
- [x] Fix asymmetry where argv-building requires `GIT_SSH` while spawn defaults to `ssh`.
- [x] Expand URL classifier use in submodule/push-submodule code:
  - [x] use full `is_configured_ssh_url` semantics for `ssh://`, `git+ssh://`, and scp-style URLs
  - [x] avoid treating SSH remotes as local paths
- [x] Preserve existing fake-SSH logging behavior used by tests.

Validation:

- [x] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5601-clone.sh`
- [ ] `./scripts/run-tests.sh t5602-clone-remote-exec.sh`
- [ ] `./scripts/run-tests.sh t5507-remote-environment.sh`
- [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`

Definition of done:

- [x] SSH command selection matches Git for env/config precedence.
- [x] Existing SSH wrapper tests do not regress.

## Phase 12: Live SSH Upload-Pack for Fetch and Clone

Dependency: Phase 11.

Primary files:

- `grit/src/ssh_transport.rs`
- `grit/src/fetch_transport.rs`
- `grit/src/commands/clone.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/ls_remote.rs`

Tasks:

- [x] Add `spawn_git_ssh_upload_pack` using shared SSH service spawning.
- [x] Expose a bidirectional upload-pack stream API in `fetch_transport.rs`.
- [~] Refactor existing upload-pack negotiation to work with:
  - [ ] local child process
  - [ ] git-daemon socket
  - [x] SSH child process
- [x] Implement live SSH `fetch` for unresolved SSH URLs.
- [x] Implement live SSH `clone` for unresolved SSH URLs.
- [x] Implement or wire live SSH `ls-remote` if not already covered.
- [x] Preserve local SSH shortcut behavior for tests that intentionally resolve to local repos.
- [x] Preserve `--upload-pack` behavior.
- [x] Preserve protocol v0/v1/v2 negotiation.
- [x] Propagate `GIT_PROTOCOL` correctly for upload-pack.
- [x] Handle child stderr/stdout/stdin closure without deadlocks.
- [x] Map SSH child exit status to Git-like errors.

Validation:

- [x] `cargo build --release -p grit-rs`
- [~] `./scripts/run-tests.sh t5601-clone.sh`
- [~] `./scripts/run-tests.sh t5603-clone-dirname.sh`
- [ ] `./scripts/run-tests.sh t5510-fetch.sh`
- [ ] `./scripts/run-tests.sh t5512-ls-remote.sh`
- [ ] `./scripts/run-tests.sh t5700-protocol-v1.sh`
- [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`

Definition of done:

- [x] SSH clone/fetch work through an external SSH process for non-local remotes.
- [ ] Grit still delegates actual SSH authentication to the user's SSH implementation.

## Phase 13: SSH Receive-Pack Hardening

Dependency: Phase 11. Prefer after Phase 12 so stream behavior is shared and well-tested.

Primary files:

- `grit/src/commands/push.rs`
- `grit/src/http_push_smart.rs`
- `grit/src/ssh_transport.rs`

Tasks:

- [ ] Re-check `push_to_ssh_url` against current receive-pack tests.
- [x] Confirm `--receive-pack` handling matches Git.
- [ ] Confirm protocol v2 receive-pack rejection is correct, or implement v2 push if tests require it.
- [ ] Validate sideband stderr/progress propagation.
- [ ] Validate push options over SSH.
- [ ] Validate atomic push over SSH.
- [ ] Validate porcelain output over SSH.
- [x] Validate hook/error propagation.
- [ ] Scrub credentials/userinfo-like data in displayed SSH URLs where applicable.
- [ ] Confirm child process cleanup on dry-run, rejected updates, and failed remote status.

Validation:

- [x] `cargo build --release -p grit-rs`
- [~] `./scripts/run-tests.sh t5545-push-options.sh`
- [ ] `./scripts/run-tests.sh t5547-push-quarantine.sh`
- [ ] `./scripts/run-tests.sh t5548-push-porcelain.sh`
- [x] `./scripts/run-tests.sh t5406-remote-rejects.sh`
- [ ] `./scripts/run-tests.sh t5409-colorize-remote-messages.sh`

Definition of done:

- [ ] SSH push behavior is covered by the same receive-pack expectations as local/HTTP push where applicable.
- [ ] Any remaining SSH push v2 limitation is documented.

## Phase 14: Protocol Policy and Security Audit

Dependency: Phases 8, 11, and 12.

Primary files:

- `grit/src/protocol.rs`
- `grit/src/commands/clone.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/push.rs`
- `grit/src/commands/submodule.rs`
- `grit-lib/src/push_submodules.rs`

Tasks:

- [~] Verify `GIT_ALLOW_PROTOCOL` behavior for HTTP, HTTPS, SSH, git, file, and ext. (`t5812` HTTP and `t5813` SSH pass; `t5814` ext disabled cases pass, enabled fetch/push remains transport-limited)
- [~] Verify `protocol.<name>.allow` behavior. (`t5812` HTTP and `t5813` SSH pass; `t5814` ext disabled cases pass, enabled fetch/push remains transport-limited)
- [~] Verify `GIT_PROTOCOL_FROM_USER` behavior. (`t5812` HTTP and `t5813` SSH pass; `t5814` ext disabled cases pass, enabled fetch/push remains transport-limited)
- [x] Ensure URL rewrites do not bypass protocol policy. (fetch/push classify after rewrite; clone/ls-remote do not currently apply rewrite rules)
- [x] Ensure submodule URL handling uses the same protocol classification as top-level operations.
- [ ] Ensure SSH path/host safety checks remain enforced before spawning subprocesses.
- [~] Ensure HTTP credentials are not sent to a different origin after redirects unless Git would do so. (`t5551` redirect/auth cases remain expected failures)
- [~] Audit trace redaction:
  - [x] `Authorization`
  - [x] `Proxy-Authorization`
  - [x] cookies
  - [x] URL userinfo
  - [x] credential helper stderr/stdout passthrough (helper stdout is parsed, not traced; helper stderr is external helper-owned output)
- [x] Document intentional security boundary: SSH auth, host keys, agents, and `~/.ssh/config` are delegated to external SSH.

Validation:

- [x] `cargo build --release -p grit-rs`
- [x] `./scripts/run-tests.sh --timeout 150 t5812-proto-disable-http.sh`
- [x] `./scripts/run-tests.sh --timeout 150 t5813-proto-disable-ssh.sh`
- [~] `./scripts/run-tests.sh --timeout 150 t5814-proto-disable-ext.sh` (19/27; remaining failures are enabled ext fetch/push transport support, not allow/deny rejection)
- [x] `./scripts/run-tests.sh --timeout 150 t5815-submodule-protos.sh`
- [x] `./scripts/run-tests.sh --timeout 150 t5581-http-curl-verbose.sh`

Definition of done:

- [~] Protocol policy tests pass or remaining failures are unrelated to auth/transport classification.
- [x] Security-sensitive auth material is redacted by default.

## Phase 15: Scope and Dashboard Cleanup

Dependency: after a test file is made meaningfully passable.

Primary files:

- `data/test-files.csv`
- `docs/index.html`
- `docs/testfiles.html`
- `docs/test-progress.svg`
- `plan.md`
- `progress.md`
- `test-results.md`
- relevant logs under `logs/`

Tasks:

- [ ] For each auth test that becomes reliable, decide whether to flip `in_scope=skip` to `in_scope=yes`.
- [ ] Refresh dashboards by running the relevant harness command or `python3 scripts/generate-dashboard-from-test-files.py`.
- [ ] Update `plan.md` checkbox/status for completed auth work.
- [ ] Update `progress.md` counts after any `plan.md` checkbox changes.
- [ ] Update `test-results.md` after meaningful cargo/harness runs.
- [ ] Add or update a timestamped log under `logs/` for each claimed implementation task.
- [ ] Remove stale status notes that contradict `data/test-files.csv`.

Validation:

- [ ] `./scripts/run-tests.sh t0300-credentials.sh`
- [ ] `./scripts/run-tests.sh t0301-credential-cache.sh`
- [ ] `./scripts/run-tests.sh t0302-credential-store.sh`
- [ ] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t5564-http-proxy.sh`
- [ ] `./scripts/run-tests.sh t5812-proto-disable-http.sh`
- [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`

Definition of done:

- [ ] Tracking files reflect the real current state.
- [ ] Auth work can be picked up by the next agent from `AUTH_TASKS.md` without rereading the entire report.

## Final Auth Milestone

Complete when all of the following are true:

- [ ] Credential protocol support is Git-compatible for core helper workflows.
- [ ] `credential-store` is fully passing.
- [ ] `credential-cache` is fully passing or explicitly deferred with an accepted reason.
- [ ] Smart HTTP auth supports Basic, helper-provided Bearer/pre-encoded credentials, challenge-aware helper calls, reject/approve, proactive auth, proxy auth, redaction, and shared bundle-uri client behavior.
- [ ] Live SSH clone/fetch/ls-remote run over external SSH upload-pack instead of requiring local resolution.
- [ ] SSH push is validated against receive-pack behavior and documented limitations.
- [ ] Protocol allow/deny and submodule transport policy remain correct.
- [ ] `data/test-files.csv`, dashboards, `plan.md`, `progress.md`, and `test-results.md` are current.
