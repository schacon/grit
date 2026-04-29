# Remote Auth Remaining Tasks

This file now tracks only unfinished remote authentication and transport-adjacent work.
Completed implementation history lives in `logs/2026-04-27_remote-auth-credential-model.md`
and validation history lives in `test-results.md`.

Work top to bottom unless a task is explicitly independent. When a task is claimed,
change `[ ]` to `[~]`; when validated or explicitly documented as deferred, change it
to `[x]` and update `test-results.md` plus the auth log.

## Current Green Auth Baseline

These suites are already in scope and fully passing in `data/test-files.csv`; do not
spend time revalidating them unless a nearby change could regress them:

- `t0300-credentials` 56/56
- `t0301-credential-cache` 52/52
- `t0302-credential-store` 65/65
- `t0303-credential-external` 23/23
- `t5541-http-push-smart` 21/21
- `t5542-push-http-shallow` 3/3
- `t5549-fetch-push-http` 3/3
- `t5555-http-smart-common` 10/10
- `t5563-simple-http-auth` 17/17
- `t5564-http-proxy` 8/8
- `t5581-http-curl-verbose` 2/2
- `t5732-protocol-v2-bundle-uri-http` 9/9
- `t5812-proto-disable-http` 29/29
- `t5813-proto-disable-ssh` 81/81
- `t5815-submodule-protos` 8/8

## 1. HTTP TLS and Proxy Transport Gaps

Dependency: HTTP client auth/proxy/cookie work is complete. These are remaining
transport capabilities in or around `grit/src/http_client.rs`.

- [ ] Implement or explicitly defer `http.sslCAInfo` / `GIT_SSL_CAINFO`.
- [ ] Implement or explicitly defer `http.sslCAPath` / `GIT_SSL_CAPATH`.
- [ ] Implement HTTPS origins through an HTTP proxy, or keep the limitation explicit:
  - [ ] send `CONNECT host:port HTTP/1.1` to the HTTP proxy
  - [ ] preserve proxy authentication and redaction
  - [ ] layer TLS to the origin after `200 Connection Established`
  - [ ] keep current HTTP-origin absolute-form proxy behavior working

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh --timeout 150 t5564-http-proxy.sh`
- [ ] Add or identify upstream-derived HTTPS proxy coverage before marking CONNECT/TLS complete.

## 2. HTTP Smart Transport Residuals

Dependency: complete or consciously defer HTTP TLS/proxy items first if they affect
the target test. These tasks are not currently believed to be auth regressions, but
they still block full HTTP auth-adjacent confidence.

- [~] Finish `t5551-http-fetch-smart.sh`.
  - Current catalog state: 29/31.
  - Remaining documented surface: redirect behavior and SHA-256 empty clone behavior.
- [~] Finish or further narrow `t5539-fetch-http-shallow.sh`.
  - Current catalog state: 4/8.
  - Remaining documented surface: shallow/deepen transport state, not auth.
- [ ] Verify auth state sharing across HTTP receive-pack:
  - [ ] receive-pack discovery
  - [ ] receive-pack POST
- [ ] Audit URL credential scrubbing in:
  - [ ] push output
  - [ ] fetch/clone error messages
  - [ ] credential helper inputs
- [~] Verify HTTP credentials are not sent to a different origin after redirects unless Git would do so.
  - Current note: `t5551` auth/redaction cases pass; redirect cases remain.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh --timeout 150 t5551-http-fetch-smart.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5539-fetch-http-shallow.sh`
- [ ] Re-run the green HTTP baseline listed above if touching shared HTTP auth/client code.

## 3. SSH Upload-Pack Broader Validation

Dependency: live SSH fetch/clone/ls-remote is implemented for unresolved SSH URLs.
These tasks are broader transport validation and shared negotiation cleanup.

- [~] Refactor existing upload-pack negotiation to work uniformly with:
  - [ ] local child process
  - [ ] git-daemon socket
  - [x] SSH child process
- [ ] Re-run and improve `t5601-clone.sh`.
  - Current catalog state: 64/115.
- [ ] Re-run and improve `t5603-clone-dirname.sh`.
  - Current catalog state: 25/47.
- [ ] Re-run and improve `t5510-fetch.sh`.
  - Current catalog state: 199/215.
- [ ] Re-run and improve `t5512-ls-remote.sh`.
  - Current catalog state: 16/40.
- [ ] Re-check `t5700-protocol-v1.sh`.
  - Current catalog state: 0/0; determine whether this is harness selection/status drift.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh --timeout 150 t5601-clone.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5603-clone-dirname.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5510-fetch.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5512-ls-remote.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5700-protocol-v1.sh`

## 4. SSH Receive-Pack and Push Hardening

Dependency: SSH command config and receive-pack basics are in place. Push-options
mostly pass; remaining work is broader push behavior and polish.

- [ ] Re-check `push_to_ssh_url` against current receive-pack tests.
- [ ] Validate sideband stderr/progress propagation.
- [ ] Validate atomic push over SSH.
- [~] Finish `t5545-push-options.sh`.
  - Current catalog state: 12/13.
  - Remaining documented surface: submodule gitlink/object validation during parent push, not push-option env propagation.
- [~] Finish or further narrow `t5548-push-porcelain.sh`.
  - Current catalog state: 5/25.
  - Remaining documented surface: broad local/HTTP push porcelain formatting gaps.
- [ ] Scrub credentials/userinfo-like data in displayed SSH URLs where applicable.
- [ ] Confirm child process cleanup on:
  - [ ] dry-run
  - [ ] rejected updates
  - [ ] failed remote status
- [ ] Decide whether SSH push behavior is sufficiently documented for the final auth milestone.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh --timeout 150 t5545-push-options.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5547-push-quarantine.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5548-push-porcelain.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5406-remote-rejects.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5409-colorize-remote-messages.sh`

## 5. Protocol Policy and Ext Transport

Dependency: HTTP, SSH, and submodule protocol policy are green. Ext disabled cases
pass; ext enabled fetch/push is transport-limited.

- [~] Finish `GIT_ALLOW_PROTOCOL` behavior for HTTP, HTTPS, SSH, git, file, and ext.
  - `t5812` HTTP and `t5813` SSH pass.
  - `t5814` ext disabled cases pass; enabled fetch/push remains transport-limited.
- [~] Finish `protocol.<name>.allow` behavior.
  - Same `t5814` limitation as above.
- [~] Finish `GIT_PROTOCOL_FROM_USER` behavior.
  - Same `t5814` limitation as above.
- [ ] Implement ext fetch/push transport support, or explicitly keep it out of the auth milestone.
- [ ] Decide whether protocol allow/deny and submodule transport policy are sufficiently documented for the final auth milestone.

Validation:

- [ ] `cargo build --release -p grit-rs`
- [ ] `./scripts/run-tests.sh --timeout 150 t5812-proto-disable-http.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5813-proto-disable-ssh.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5814-proto-disable-ext.sh`
- [ ] `./scripts/run-tests.sh --timeout 150 t5815-submodule-protos.sh`

## 6. Tracking and Handoff Cleanup

Dependency: do this after any task above changes status or meaningfully changes test
results. This is the final cleanup needed before the auth work can be considered
fully handed off.

- [ ] Resolve status drift between `plan.md`, `progress.md`, and `data/test-files.csv`.
- [ ] Update `plan.md` checkbox/status for completed auth work.
- [ ] Update `progress.md` counts after any `plan.md` checkbox changes.
- [ ] Update `test-results.md` after meaningful cargo/harness runs.
- [ ] Add or update a timestamped log under `logs/` for each claimed implementation task.
- [ ] Remove stale status notes that contradict `data/test-files.csv`.
- [ ] Ensure `data/test-files.csv`, dashboards, `plan.md`, `progress.md`, and `test-results.md` are current.
- [ ] Ensure auth work can be picked up from this file without rereading the full historical report.

Final milestone blockers:

- [ ] SSH push is validated against receive-pack behavior and documented limitations.
- [ ] Protocol allow/deny and submodule transport policy remain correct.
- [ ] Tracking files reflect the real current state.
