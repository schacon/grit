# Test results

**2026-04-10 (fetch tail completion + full t5510 green)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `cargo build --release -p grit-rs`: pass
- `GUST_BIN=/workspace/target/release/grit bash tests/t5510-fetch.sh -v`: **215/215**
- `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh -v`: **24/24**
- Regression matrix checkpoint:
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **6/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
  - `data/test-files.csv` refreshed and dashboards regenerated (`docs/index.html`, `docs/testfiles.html`, `docs/test-progress.svg`).
- Final fixes in this completion slice:
  - fetch connectivity trace parity for hideRefs (`--exclude-hidden=fetch`) now emitted in upload-pack path.
  - auto-gc message parity in `gc --auto` restored for fetch-triggered auto maintenance.
  - unpack-limit storage parity for fetch now honors `fetch.unpacklimit` over `transfer.unpacklimit` and stores packs via `index-pack` when threshold is met.
  - bundle `--since/--until` date parsing aligned with Git human date format for rev filtering.
  - boundary-bundle object selection tuned to preserve thin-pack expected object count in `t5510.187`.

**2026-04-10 (fetch bundle/parity follow-up + prune tail progress)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5510-fetch.sh`: **209/215**
  - improved from 195/215 at this turn start.
  - newly fixed in this iteration:
    - bundle option/header/list-heads parity (`43`, `44`, `47`, `56`)
    - bundle path fetch behavior (`48`)
    - fetch.writeCommitGraph + submodule variant (`74`, `75`)
    - prune output URL parity (`188`)
    - branchname D/F conflict resolved by `--prune` (`189`)
- Remaining failures in `t5510-fetch.sh`:
  - `187`, `190`, `192`, `193`, `194`, `196`
- Key code changes:
  - `bundle create` now supports `--version=3` header behavior and prerequisite subject lines.
  - `bundle create` object selection for `-<n>` now excludes parent-commit tree payload for unchanged objects.
  - `bundle list-heads` now prints canonical full ref names (`refs/heads/<name>`).
  - `fetch` supports path-based bundle files via `bundle unbundle` when explicitly fetched.
  - `fetch` now honors `fetch.writeCommitGraph` by invoking `commit-graph write` post-fetch.
  - `fetch` source resolution and tag-follow wants were refined for CLI refspec parity.

**2026-04-10 (fetch atomic transaction + prune/lock parity pass)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5510-fetch.sh`: **178/215** (improved from 174/215 before this iteration)
- `GUST_BIN=/workspace/target/release/grit bash tests/t5510-fetch.sh -v`: confirms atomic cluster progress
  - now passing: 31, 33, 34, 35
  - remaining in atomic cluster: 32 (`reference-transaction` expected extra preparing line for `refs/remotes/origin/HEAD`)
- Key changes in this increment:
  - staged + transactional `--atomic` ref updates/deletes in fetch (single apply point + hook phases),
  - packed-refs rewrite lock path switched to `packed-refs.new` with `create_new(true)` semantics,
  - loose ref and symref lockfiles now use create-new lock semantics,
  - prune semantics aligned for explicit CLI refspecs vs prune-tags/pruneTags config interactions.

**2026-04-10 (fetch glob wants + prune scope refinement)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `./scripts/run-tests.sh t5510-fetch.sh`: 90/215
  - Improved from 86/215 before this increment (and 43/215 earlier in the same run).
  - Key changes:
    - upload-pack wants now support glob refspec expansion from advertised refs (no hard failure),
    - prune scope for CLI/configured refspecs now derives from destination namespaces instead of
      pruning all `refs/remotes/<remote>/*` unconditionally.
- Regression matrix after this increment:
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: 20/24
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 27/37
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: 6/16
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: 10/16
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: 10/10
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: 0/0 (harness timeout mode in this run)

**2026-04-10 (regression matrix refresh after protocol-v1 + no-MIDX fixes)**

- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 20/24
  - Remaining failures are ssh:// protocol-v1 tests (14/15/16/17).
  - file:// and http:// protocol-v1 paths now pass in focused and suite runs.
- `./scripts/run-tests.sh t5510-fetch.sh`: 43/215 (improved from earlier 23/215 in this session)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 27/37 (improved from earlier 21/37)
- `./scripts/run-tests.sh t5537-fetch-shallow.sh`: 6/16 (improved from earlier 1/16)
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: 10/16 (unchanged from latest baseline)

**2026-04-10 (protocol-v1 file/http transport and MIDX reuse guard)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=6,7,11`: all passed
  - `clone with file:// using protocol v1` now passes.
  - `cloning branchless tagless but not refless remote` now passes (no more `no multi-pack-index found`).
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=6,7,8,9,10,13,14,15,16,17,19,20,22`:
  - file:// + http:// subset in this run: pass
  - remaining failures: ssh:// protocol-v1 tests 14/15/16/17
- Code changes in this increment:
  - `pack-objects` MIDX reuse path now treats missing multi-pack-index as "reuse unavailable" instead of fatal,
    avoiding clone/fetch failure on repositories without MIDX files.

**2026-04-10 (fetch HTTP v1 retry loop + protocol-v1 checkpoint)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=83,84,85`: 85/85 passed
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 14/24 (improved from prior 9/24 baseline in this environment)
- Focused repro:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`
  - Passed: 19,20
  - Remaining failure: 22
- Code changes in this increment:
  - HTTP v0/v1 stateless fetch now retries once without `have` lines when initial response yields no pack while
    wanted objects are missing locally.
  - HTTP v0/v1 stateless fetch request framing now flushes after the `want` section before negotiation
    (`have` / `done`), matching expected v1 stateless upload-pack boundaries.
  - Side-band parser improvements retained for PACK boundary handling and pre-pack flush tolerance.

**2026-04-10 (fetch HTTP v1 wants/flush framing follow-up)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`: passed
  - 19,20,22 all pass after v1 wants/flush boundary fix.
- `./scripts/run-tests.sh t5700-protocol-v1.sh`: 15/24
- `./scripts/run-tests.sh t5702-protocol-v2.sh`: 0/0 (harness timeout mode in this run)
- Extended matrix checkpoint after this increment:
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: 10/10
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 21/37 (improved from 13/37)
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: 1/16 (improved from 0/16)
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: 10/16 (improved from 0/16)
  - `./scripts/run-tests.sh t5510-fetch.sh`: 23/215 (improved from 16/215)

**2026-04-10 (fetch HTTP v1 setup + sideband parsing hardening)**

- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=83,84,85`: 85/85 passed
  - `http:// --negotiate-only without wait-for-done support` now passes reliably in this environment.
- `GUST_BIN=/workspace/target/release/grit bash tests/t5700-protocol-v1.sh --run=19,20,22`: partial
  - Passed: 19 (`create repos`), 20 (`clone with http:// using protocol v1`)
  - Remaining failure: 22 (`fetch with http:// using protocol v1`) still fails; logs show refs update output but
    post-fetch validation still does not see `origin/main` as expected in this harness flow.
- Code changes in this increment:
  - Removed `git add` auto-root-commit behavior that was causing setup repos to report
    `error: nothing to commit, working tree clean` in protocol/http harness setup.
  - Hardened HTTP v0/v1 side-band pack reader in `http_smart`:
    - avoid pre-consuming the first binary data pkt-line before sideband demux,
    - tolerate pre-pack flushes,
    - detect `PACK` magic across pkt boundaries/channels.

**2026-04-10 (fetch-plan Phase E.4: one_time_script HTTP route parity)**
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: 166 passed
- `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=83,84,85`: partial
  - Passed: 83,85
  - Remaining failure: 84 still fails in this environment despite implementing `/one_time_script/*`
    routing in `test_httpd`; verbose run shows setup creating `server/` outside HTTP docroot
    (`$HTTPD_DOCUMENT_ROOT_PATH`) and then cloning from `/one_time_script/server`, so the endpoint
    resolves to a missing repository path before the wait-for-done assertion is reached.
- Regression checkpoint:
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: 9/24
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: 13/37
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: 10/10
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: 0/16 (pre-existing; `git http-backend`
    still intentionally unimplemented in grit command path)

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
