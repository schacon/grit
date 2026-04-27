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
