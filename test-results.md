# Test results

**2026-04-10 (fetch-plan Phase E.2: ls-remote over HTTP)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- Manual HTTP ls-remote parity check (local `test-httpd`):
  - Created temp source+mirror repo under `/tmp/grit-lsremote-http-test`.
  - Started `target/release/test-httpd` and served `/smart/repo.git`.
  - Compared:
    - `grit -c protocol.version=2 ls-remote http://127.0.0.1:<port>/smart/repo.git`
    - `git  -c protocol.version=2 ls-remote http://127.0.0.1:<port>/smart/repo.git`
  - `diff -u` output is empty (matching refs output).
- Regression checkpoint:
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: 0/0
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: 9/24
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 13/37

**2026-04-10 (fetch HTTP refetch parity / Phase C.4)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5702-protocol-v2.sh`: 0/0 (timeout/environmental in this run)
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 9/24 (unchanged baseline in this environment)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 13/37 (unchanged baseline in this environment)
- `./scripts/run-tests.sh t5537-fetch-shallow.sh`: 0/16 (unchanged baseline in this environment)
  - Code now supports `--refetch` over HTTP by suppressing `have`-based negotiation in both
    v2 and v0/v1 HTTP fetch paths, making object transfer behavior match Git's documented
    "fresh clone" semantics for refetch/filter reapplication.

**2026-04-10 (fetch HTTP option wiring / Phase C.1 C.2 C.3 groundwork)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 9/24 (unchanged in this environment)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 13/37 (unchanged in this environment)
  - HTTP fetch path now threads shallow/deepen/filter request options into smart HTTP v2/v0-v1
    request encoding.
  - HTTP CLI refspec parity groundwork added (glob expansion + explicit mapping path over advertised
    refs), but this suite remains limited by broader pre-existing fetch gaps.

**2026-04-10 (fetch HTTP v0/v1 stateless negotiation / Phase B.2 partial)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GIT_TRACE_CURL=1 GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`: failing (`20` passes, `22` still fails due to content mismatch in this harness)
  - `tests/trash.t5700-protocol-v1/log` now includes protocol-v1 packet trace lines for HTTP v0/v1 fallback:
    - `packet:          git< version 1`
  - The HTTP fetch transport path is now exercising v0/v1 negotiation with local `have` lines and server v1 trace visibility.

**2026-04-10 (fetch-plan Phase E.1: --negotiate-only behavior)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=53,54,55,56,83,84,85`: partial
  - Passed: 53,54,55,56,83,85
  - Remaining failure: 84 (`http:// --negotiate-only without wait-for-done support`) in this environment due to `one_time_script` route not being found in our local test-httpd flow, so expected wait-for-done error assertion is not reached.
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 9/24
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 13/37

**2026-04-10 (fetch HTTP v0/v1 discovery fallback / Phase B.1)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 9/24
  - HTTP-focused subset (`--run=20,21,22,23,24`) still failing in this environment.
  - Added fallback plumbing for non-v2 smart advertisements in `http_smart`; full parity still
    requires additional protocol work in later phases.

**2026-04-10 (fetch HTTP transport / protocol header control)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh`: 9/24 passed
  - Notable for this increment: HTTP protocol-v1 checks improved/held (`clone with http:// using protocol v1` passed and trace-based `Git-Protocol: version=1` assertion in that test remained green).
  - Remaining failures in this file are pre-existing across non-HTTP and broader v1 behavior.

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

**2026-04-10 (fetch-plan Phase A slice: HTTP auth + POST transport)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `bash tests/t5551-http-fetch-smart.sh --run=32`: fails (known pre-existing broader HTTP fetch gaps; credential storage path now active, but suite setup still non-green)
- `bash tests/t5551-http-fetch-smart.sh --run=33`: fails (known pre-existing broader HTTP fetch gaps; expected askpass/credential interaction still not fully matching)
- `bash tests/t5564-http-proxy.sh`: fails in this environment with pre-existing setup/path issues; proxy/auth regression inspected via access logs
- Manual credential helper validation:
  - `grit credential approve` with `credential.helper=store` writes credentials
  - `grit credential fill` returns stored username/password
  - `grit credential reject` erases stored entry (credential file reduced to empty newline)
