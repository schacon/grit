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

## Phase 1: Credential Data Model [~]

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
- [ ] Add protection against invalid protocol/host fields required by `credential.protectProtocol`.
- [ ] Add prompt-safe rendering helpers for usernames/hosts required by `credential.sanitizePrompt`.
- [ ] Drive validation from the upstream-derived harness files; do not add new non-upstream tests for this work.

Validation:

- [x] `cargo test -p grit-lib --lib`
- [x] `cargo build --release -p grit-rs`
- [~] `./scripts/run-tests.sh t0300-credentials.sh` (attempted; skipped by current `data/test-files.csv` scope)

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

- [ ] Implement `grit credential capability`.
- [ ] Match Git helper chain behavior for `fill`:
  - [ ] invoke helpers in config load order
  - [ ] support empty helper reset
  - [ ] stop once username/password are complete
  - [ ] stop once `authtype` + `credential` are complete
  - [ ] stop on `quit=true` or `quit=1`
  - [ ] continue through partial helper responses
- [ ] Implement helper output filtering based on caller capabilities:
  - [ ] only accept `authtype` / `credential` when caller sent `capability[]=authtype`
  - [ ] only accept `state[]` / `continue` when caller sent `capability[]=state`
- [ ] Implement `password_expiry_utc` handling:
  - [ ] ignore expired passwords during `fill`
  - [ ] preserve non-expired values where Git does
- [ ] Preserve `oauth_refresh_token` as confidential helper data.
- [ ] Honor `ephemeral`:
  - [ ] do not persist ephemeral credentials in helpers that should not store them
  - [ ] still notify helpers on approve/reject when Git would
- [ ] Implement `credential.interactive=false`.
- [ ] Implement prompt fallback order:
  - [ ] `GIT_ASKPASS`
  - [ ] `core.askPass`
  - [ ] `SSH_ASKPASS`
  - [ ] terminal prompt when interactive is allowed
- [ ] Make failure messages match Git closely enough for `t0300`.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t0300-credentials.sh`
- [ ] `./scripts/run-tests.sh t0303-credential-external.sh`

Definition of done:

- [ ] `t0300-credentials` is mostly or fully passing.
- [ ] `t0303-credential-external` remains passing.
- [ ] Credential helpers can return Bearer-style credentials for later HTTP use.

## Phase 3: Credential Store Parity

Dependency: Phase 1. Can proceed in parallel with Phase 2 only after the shared credential parser is stable.

Primary files:

- `grit/src/commands/credential_store.rs`
- `grit/src/commands/credential.rs`

Tasks:

- [ ] Implement default file search order:
  - [ ] `~/.git-credentials`
  - [ ] `$XDG_CONFIG_HOME/git/credentials`
  - [ ] `$HOME/.config/git/credentials` when `XDG_CONFIG_HOME` is unset or empty
- [ ] Implement default write target:
  - [ ] first existing file among Git's default list
  - [ ] create `~/.git-credentials` if none exists
- [ ] Support `--file <path>` and `--file=<path>`.
- [ ] Implement URL parsing for credential-store lines with Git-compatible invalid-line handling.
- [ ] Match entries by:
  - [ ] protocol
  - [ ] host and optional port
  - [ ] username when supplied in the query
  - [ ] path when path is relevant
- [ ] Respect `credential.useHttpPath`.
- [ ] Handle CRLF rules from `t0302`.
- [ ] Erase matching credentials from all relevant files.
- [ ] Avoid duplicate stored credentials when replacing/updating existing entries.
- [ ] Preserve or set restrictive file permissions on Unix.
- [ ] Decide and document behavior for unreadable store files to match Git tests.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t0302-credential-store.sh`
- [ ] `./scripts/run-tests.sh t0300-credentials.sh`

Definition of done:

- [ ] `t0302-credential-store` is fully passing.
- [ ] Store helper behavior remains compatible with credential helper chaining.

## Phase 4: Credential Cache Daemon

Dependency: Phase 1. Prefer after Phase 2 because cache semantics share expiry/capability behavior.

Primary files:

- `grit/src/commands/credential_cache.rs`
- possible daemon subcommand/module if needed

Tasks:

- [ ] Design a minimal Git-compatible Unix socket cache daemon.
- [ ] Implement default socket path selection:
  - [ ] `$XDG_CACHE_HOME/git/credential/socket`
  - [ ] `$HOME/.cache/git/credential/socket`
  - [ ] `$HOME/.git-credential-cache/socket` when that directory exists
- [ ] Support `--socket <path>` and `--socket=<path>`.
- [ ] Reject or error on relative socket paths if Git does.
- [ ] Implement `store` over the daemon protocol.
- [ ] Implement `get` with matching semantics from the credential model.
- [ ] Implement `erase`.
- [ ] Implement `exit`.
- [ ] Implement timeout expiration from `--timeout`.
- [ ] Honor `password_expiry_utc`.
- [ ] Preserve confidential fields such as `oauth_refresh_token` if Git cache tests require it.
- [ ] Enforce socket directory permissions sufficiently for upstream tests.
- [ ] Ensure daemon cleanup does not leave stale background processes after tests.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t0301-credential-cache.sh`
- [ ] `./scripts/run-tests.sh t0300-credentials.sh`

Definition of done:

- [ ] `t0301-credential-cache` is fully passing on Unix-like platforms.
- [ ] Cache daemon handles repeated helper invocations without leaking stale credentials.

## Phase 5: HTTP Auth Challenge Parsing

Dependency: Phases 1 and 2.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/commands/credential.rs`
- possible new `grit/src/http_auth.rs`

Tasks:

- [ ] Extend raw HTTP response capture to retain response headers, not just status/reason/body.
- [ ] Capture all `WWW-Authenticate` headers on `401`.
- [ ] Parse header names case-insensitively.
- [ ] Support multiple challenge headers in order.
- [ ] Support folded/continued header lines for test compatibility.
- [ ] Preserve challenge strings for `wwwauth[]` exactly enough for helpers/tests.
- [ ] Pass `capability[]=authtype` and `capability[]=state` to `credential fill`.
- [ ] Pass all parsed challenges as ordered `wwwauth[]`.
- [ ] Include relevant `wwwauth[]` and state fields in `approve` / `reject`.
- [ ] Keep current Basic username/password flow working when no advanced auth is returned.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t0300-credentials.sh`

Definition of done:

- [ ] HTTP auth helpers receive Git-compatible challenge input.
- [ ] Basic auth tests still pass through the new challenge-aware path.

## Phase 6: HTTP Auth Schemes and Multistage Flow

Dependency: Phase 5.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/http_smart.rs`
- `grit/src/http_push_smart.rs`

Tasks:

- [ ] Represent resolved HTTP auth as an enum rather than Basic-only username/password:
  - [ ] Basic from username/password
  - [ ] pre-encoded `authtype` + `credential`
  - [ ] empty auth if supported later
- [ ] Build `Authorization: <authtype> <credential>` from helper-provided credentials.
- [ ] Preserve Basic `Authorization` generation for username/password.
- [ ] Implement invalid credential retry/reject behavior:
  - [ ] reject failed credentials with all relevant credential fields
  - [ ] clear in-process auth cache on failure
  - [ ] avoid reusing stale credentials across RPC requests
- [ ] Implement `continue=1` multistage auth:
  - [ ] call helpers again with `state[]`
  - [ ] pass updated challenges
  - [ ] cap retry loops to avoid infinite authentication loops
- [ ] Avoid storing credentials marked `ephemeral` where Git avoids persistence.
- [ ] Share auth state across smart HTTP discovery and RPC POSTs in one operation.
- [ ] Make GET and POST behavior consistent.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t5555-http-smart-common.sh`
- [ ] `./scripts/run-tests.sh t5549-fetch-push-http.sh`

Definition of done:

- [ ] `t5563-simple-http-auth` passes Basic, Bearer, invalid credentials, and multistage cases that are supported by the test environment.
- [ ] Existing unauthenticated smart HTTP tests do not regress.

## Phase 7: Proactive and Empty HTTP Auth

Dependency: Phase 6.

Primary files:

- `grit/src/http_client.rs`
- `grit-lib/src/config.rs` if config parsing needs additions

Tasks:

- [ ] Parse `http.proactiveAuth` values:
  - [ ] `basic`
  - [ ] `auto`
  - [ ] `none`
- [ ] Implement proactive Basic:
  - [ ] call credential helpers before first request
  - [ ] request Basic-capable credentials from helpers
  - [ ] send `Authorization` on first request
- [ ] Implement proactive auto:
  - [ ] allow helper-selected auth scheme
  - [ ] fall back to Basic only when Git would
- [ ] Parse and implement `http.emptyAuth`.
- [ ] Ensure proactive auth is disabled by default.
- [ ] Ensure credentials are not sent over plain HTTP unexpectedly beyond Git-compatible behavior.
- [ ] Update trace redaction for proactive auth headers.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`

Definition of done:

- [ ] Proactive Basic and auto auth cases in `t5563-simple-http-auth` pass.
- [ ] Auth trace output remains redacted by default.

## Phase 8: HTTP Request Configuration Parity

Dependency: can begin after Phase 6; keep separate from challenge auth to avoid mixing failures.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/commands/clone.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/push.rs`
- `grit/src/commands/ls_remote.rs`

Tasks:

- [ ] Implement `http.extraHeader`:
  - [ ] multiple values
  - [ ] empty-value reset
  - [ ] per-URL matching if config layer supports it
  - [ ] redaction for auth-like headers in traces
- [ ] Implement environment proxy variables:
  - [ ] `http_proxy`
  - [ ] `https_proxy`
  - [ ] `all_proxy`
  - [ ] `no_proxy`
  - [ ] Git-compatible precedence with `http.proxy`
- [ ] Implement `http.proxyAuthMethod`.
- [ ] Implement `GIT_HTTP_PROXY_AUTHMETHOD`.
- [ ] Handle proxy `407` / `Proxy-Authenticate` enough for tests.
- [ ] Audit current manual HTTP forward proxy path for HTTPS behavior and document any limitation.
- [ ] Add `remote.<name>.proxy` if required by tests encountered in this phase.
- [ ] Make proxy auth redaction match `GIT_TRACE_REDACT`.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5564-http-proxy.sh`
- [ ] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`
- [ ] `./scripts/run-tests.sh t5555-http-smart-common.sh`

Definition of done:

- [ ] `t5564-http-proxy` no longer times out and has clear pass/fail counts.
- [ ] Proxy credentials are never leaked in default traces.

## Phase 9: HTTP Cookies, TLS, and Split HTTP Stack

Dependency: Phase 8 for request configuration; Phase 6 for authenticated requests.

Primary files:

- `grit/src/http_client.rs`
- `grit/src/bundle_uri.rs`
- `grit/src/http_bundle_uri.rs`
- `grit/src/commands/http_fetch.rs`
- `grit/src/commands/http_push.rs`

Tasks:

- [ ] Upgrade `http.cookieFile` support:
  - [ ] Netscape cookie format
  - [ ] domain matching
  - [ ] path matching
  - [ ] secure flag handling where applicable
  - [ ] simplified header format remains supported
- [ ] Implement `http.saveCookies`.
- [ ] Implement TLS-related configuration that the current HTTP stack can support:
  - [ ] `http.sslVerify`
  - [ ] `GIT_SSL_NO_VERIFY`
  - [ ] `http.sslCAInfo` / `GIT_SSL_CAINFO`
  - [ ] `http.sslCAPath` / `GIT_SSL_CAPATH` if feasible
  - [ ] document unsupported client certificate options if `ureq` cannot support them cleanly
- [ ] Audit `http.sslCert`, `http.sslKey`, and password-protected cert behavior.
- [ ] Route bundle URI HTTP(S) downloads through `HttpClientContext`:
  - [ ] `grit/src/bundle_uri.rs`
  - [ ] `grit/src/http_bundle_uri.rs`
  - [ ] preserve existing bundle-uri protocol behavior
  - [ ] ensure auth, proxy, cookies, and trace are shared with normal HTTP remote operations
- [ ] Audit other raw `ureq` uses and either route through shared client or document why not.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5732-protocol-v2-bundle-uri-http.sh`
- [ ] `./scripts/run-tests.sh t5551-http-fetch-smart.sh`
- [ ] `./scripts/run-tests.sh t5563-simple-http-auth.sh`
- [ ] `./scripts/run-tests.sh t5564-http-proxy.sh`

Definition of done:

- [ ] Authenticated/proxied bundle URI fetches use the same client behavior as normal HTTP remotes.
- [ ] TLS support and limitations are explicit and covered by tests where feasible.

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

- [ ] Re-run unauthenticated HTTP baseline and fix regressions.
- [ ] Re-run authenticated HTTP tests and fix regressions.
- [ ] Verify auth state sharing across:
  - [ ] discovery GET
  - [ ] ls-refs POST
  - [ ] fetch POST
  - [ ] receive-pack discovery
  - [ ] receive-pack POST
- [ ] Verify redirect/auth behavior in `t5551-http-fetch-smart`.
- [ ] Verify shallow HTTP fetch/push behavior was not broken by auth changes.
- [ ] Audit URL credential scrubbing in:
  - [ ] push output
  - [ ] fetch/clone errors
  - [ ] trace output
  - [ ] credential helper inputs

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5555-http-smart-common.sh`
- [ ] `./scripts/run-tests.sh t5549-fetch-push-http.sh`
- [ ] `./scripts/run-tests.sh t5551-http-fetch-smart.sh`
- [ ] `./scripts/run-tests.sh t5541-http-push-smart.sh`
- [ ] `./scripts/run-tests.sh t5539-fetch-http-shallow.sh`
- [ ] `./scripts/run-tests.sh t5542-push-http-shallow.sh`
- [ ] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`

Definition of done:

- [ ] Smart HTTP auth is integrated across fetch, clone, ls-remote, and push.
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

- [ ] Add `core.sshCommand` support.
- [ ] Implement precedence:
  - [ ] `GIT_SSH_COMMAND`
  - [ ] `core.sshCommand`
  - [ ] `GIT_SSH`
  - [ ] default `ssh`
- [ ] Audit `GIT_SSH_VARIANT` vs `ssh.variant` precedence.
- [ ] Keep OpenSSH/Plink/Putty/TortoisePlink/simple variant handling compatible.
- [ ] Fix asymmetry where argv-building requires `GIT_SSH` while spawn defaults to `ssh`.
- [ ] Expand URL classifier use in submodule/push-submodule code:
  - [ ] use full `is_configured_ssh_url` semantics for `ssh://`, `git+ssh://`, and scp-style URLs
  - [ ] avoid treating SSH remotes as local paths
- [ ] Preserve existing fake-SSH logging behavior used by tests.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5601-clone.sh`
- [ ] `./scripts/run-tests.sh t5602-clone-remote-exec.sh`
- [ ] `./scripts/run-tests.sh t5507-remote-environment.sh`
- [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`

Definition of done:

- [ ] SSH command selection matches Git for env/config precedence.
- [ ] Existing SSH wrapper tests do not regress.

## Phase 12: Live SSH Upload-Pack for Fetch and Clone

Dependency: Phase 11.

Primary files:

- `grit/src/ssh_transport.rs`
- `grit/src/fetch_transport.rs`
- `grit/src/commands/clone.rs`
- `grit/src/commands/fetch.rs`
- `grit/src/commands/ls_remote.rs`

Tasks:

- [ ] Add `spawn_git_ssh_upload_pack` using shared SSH service spawning.
- [ ] Expose a bidirectional upload-pack stream API in `fetch_transport.rs`.
- [ ] Refactor existing upload-pack negotiation to work with:
  - [ ] local child process
  - [ ] git-daemon socket
  - [ ] SSH child process
- [ ] Implement live SSH `fetch` for unresolved SSH URLs.
- [ ] Implement live SSH `clone` for unresolved SSH URLs.
- [ ] Implement or wire live SSH `ls-remote` if not already covered.
- [ ] Preserve local SSH shortcut behavior for tests that intentionally resolve to local repos.
- [ ] Preserve `--upload-pack` behavior.
- [ ] Preserve protocol v0/v1/v2 negotiation.
- [ ] Propagate `GIT_PROTOCOL` correctly for upload-pack.
- [ ] Handle child stderr/stdout/stdin closure without deadlocks.
- [ ] Map SSH child exit status to Git-like errors.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5601-clone.sh`
- [ ] `./scripts/run-tests.sh t5603-clone-dirname.sh`
- [ ] `./scripts/run-tests.sh t5510-fetch.sh`
- [ ] `./scripts/run-tests.sh t5512-ls-remote.sh`
- [ ] `./scripts/run-tests.sh t5700-protocol-v1.sh`
- [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`

Definition of done:

- [ ] SSH clone/fetch work through an external SSH process for non-local remotes.
- [ ] Grit still delegates actual SSH authentication to the user's SSH implementation.

## Phase 13: SSH Receive-Pack Hardening

Dependency: Phase 11. Prefer after Phase 12 so stream behavior is shared and well-tested.

Primary files:

- `grit/src/commands/push.rs`
- `grit/src/http_push_smart.rs`
- `grit/src/ssh_transport.rs`

Tasks:

- [ ] Re-check `push_to_ssh_url` against current receive-pack tests.
- [ ] Confirm `--receive-pack` handling matches Git.
- [ ] Confirm protocol v2 receive-pack rejection is correct, or implement v2 push if tests require it.
- [ ] Validate sideband stderr/progress propagation.
- [ ] Validate push options over SSH.
- [ ] Validate atomic push over SSH.
- [ ] Validate porcelain output over SSH.
- [ ] Validate hook/error propagation.
- [ ] Scrub credentials/userinfo-like data in displayed SSH URLs where applicable.
- [ ] Confirm child process cleanup on dry-run, rejected updates, and failed remote status.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5545-push-options.sh`
- [ ] `./scripts/run-tests.sh t5547-push-quarantine.sh`
- [ ] `./scripts/run-tests.sh t5548-push-porcelain.sh`
- [ ] `./scripts/run-tests.sh t5406-remote-rejects.sh`
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

- [ ] Verify `GIT_ALLOW_PROTOCOL` behavior for HTTP, HTTPS, SSH, git, file, and ext.
- [ ] Verify `protocol.<name>.allow` behavior.
- [ ] Verify `GIT_PROTOCOL_FROM_USER` behavior.
- [ ] Ensure URL rewrites do not bypass protocol policy.
- [ ] Ensure submodule URL handling uses the same protocol classification as top-level operations.
- [ ] Ensure SSH path/host safety checks remain enforced before spawning subprocesses.
- [ ] Ensure HTTP credentials are not sent to a different origin after redirects unless Git would do so.
- [ ] Audit trace redaction:
  - [ ] `Authorization`
  - [ ] `Proxy-Authorization`
  - [ ] cookies
  - [ ] URL userinfo
  - [ ] credential helper stderr/stdout passthrough
- [ ] Document intentional security boundary: SSH auth, host keys, agents, and `~/.ssh/config` are delegated to external SSH.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh t5812-proto-disable-http.sh`
- [ ] `./scripts/run-tests.sh t5813-proto-disable-ssh.sh`
- [ ] `./scripts/run-tests.sh t5814-proto-disable-ext.sh`
- [ ] `./scripts/run-tests.sh t5815-submodule-protos.sh`
- [ ] `./scripts/run-tests.sh t5581-http-curl-verbose.sh`

Definition of done:

- [ ] Protocol policy tests pass or remaining failures are unrelated to auth/transport classification.
- [ ] Security-sensitive auth material is redacted by default.

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
