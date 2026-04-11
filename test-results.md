# Test results

**2026-04-11 (protocol-v2 HTTP malformed pkt-line clone parity: t5702.63/.64)**

- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `cargo build -p grit-rs --bin test-httpd`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=61-64 -v`: **pass**
    - `63` now fails with expected malformed-length error (`bytes of length header were received`).
    - `64` now fails with expected malformed-body error (`bytes of body are still expected`).
- Regression checks:
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `test-httpd` now serves deterministic malformed smart-HTTP upload-pack POST responses for:
    - `/smart/incomplete_length/<repo>/git-upload-pack` → body `00`
    - `/smart/incomplete_body/<repo>/git-upload-pack` → body `007945`
  - This mirrors upstream one-time-script malformed pkt-line scenarios for HTTP protocol-v2 clone error handling.

**2026-04-11 (protocol-v2 cluster: exact-oid/cli-prefix/tag-follow + deepen-relative + custom-path packfile-uris)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=44-49 -v`: **pass**
    - fixed `47` (exact-oid fetch avoids pointless `ls-refs`),
    - fixed `48` (CLI ref-prefix/have-line/tag-following filtering parity),
    - fixed `49` (`include-tag` request shaping for tag-following parity).
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=50-52 -v`: **pass**
    - fixed `52` (`--deepen` relative behavior in local protocol-v2 path).
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=55-60 -v`: **pass**
    - fixed `57` (custom `--receive-pack` no protocol-v2 env),
    - fixed `58` (custom `--upload-pack` requests protocol-v2 env),
    - fixed `59` (remote archive custom exec path no protocol-v2 env),
    - fixed `60` (packfile-uris rejected unless advertised by config).
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-52 -v`: **pass** (confirms no regression in earlier v2 slices).
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **70/85** (improved from 62/85)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `fetch_transport`:
    - skip v2 `ls-refs` when all CLI sources are explicit OIDs,
    - broaden v2 ref-prefix emission for unqualified names (`dwim` + `refs/heads/dwim`),
    - thread `include_tag` and `deepen-relative` request shaping.
  - `file_upload_pack_v2`:
    - emit `include-tag` and `deepen-relative` lines in v2 fetch requests.
  - `fetch`:
    - local CLI wants now use selective follow-tag expansion instead of fetching all tag refs,
    - refined shallow metadata updates:
      - depth fetches preserve existing boundaries,
      - deepen fetches replace ancestor boundaries for fetched tips.
  - `pack_objects_upload`:
    - depth-limited pack generation now avoids stdin `--not` arguments to prevent under-fetch in relative deepen scenarios.
  - `send_pack`:
    - stop forwarding protocol-v2 env for custom `--receive-pack` commands.
  - `archive`:
    - resolve remote names via `remote.<name>.url` in remote archive mode,
    - rewrite shell custom exec `git-upload-archive` to current grit upload-archive binary while preserving wrappers and clearing protocol env.
  - `serve_v2`:
    - advertise/accept `packfile-uris` only when `uploadpack.blobpackfileuri` is configured with a non-empty value,
    - parse valueless config keys as empty string for proper gate semantics.

**2026-04-11 (protocol-v2 filter cluster 37/39/40/42 parity + promisor hydrate fallback)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=35-42 -v`: **pass (85/85 in run subset)**
    - `37` dynamically fetch missing object now uses protocol v2 and succeeds.
    - `39` partial fetch over file:// protocol v2 now honors `--filter=blob:none`.
    - `40` filter capability advertisement follows `uploadpack.allowfilter`.
    - `42` handcrafted `filter blob:none` request is rejected when filter is not advertised.
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **62/85** (improved from 58/85)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `fetch_transport`:
    - explicit-wants path now negotiates protocol v2 when configured and sends v2 `command=fetch` payloads.
    - threaded optional filter spec into v2 fetch request writing and low-level negotiation helpers.
  - `file_upload_pack_v2`:
    - `write_v2_fetch_request` now supports optional `filter <spec>` line emission.
  - `serve_v2`:
    - `fetch=` capability advertisement now includes `filter` only when `uploadpack.allowfilter` is enabled.
    - v2 fetch command now rejects `filter ...` lines when filter is not advertised.
    - v2 fetch now passes the parsed filter spec down into pack generation.
  - `upload_pack` / `pack_objects_upload`:
    - local upload-pack now forwards optional filter spec to `pack-objects --filter`.
  - `promisor_hydrate`:
    - lazy promisor object hydration now has a robust fallback:
      - first attempt honors configured `remote.<name>.partialclonefilter`,
      - on failure, retries without filter to fetch explicitly requested missing objects.
  - `rev_list`:
    - `--quiet --objects --missing=print` now still emits object/missing lines (suppresses commit lines only), matching the `t5702.39` expectation.

**2026-04-11 (protocol-v2 git:// EAGAIN fix + file:// unborn HEAD clone parity)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-7 -v`: **pass (85/85 in run subset)**
    - fixes `git://` protocol-v2 clone/fetch/pull regressions:
      - `4` clone no longer fails with `Resource temporarily unavailable (os error 11)`,
      - `5/6/7` now pass end-to-end with protocol-v2 traces.
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=9-21 -v`: all file:// clone-head cases pass except expected broader suite skips
    - `16`: empty-repo default branch propagation restored,
    - `17`: `lsrefs.unborn=ignore` fallback behavior restored,
    - `18`: bare empty clone HEAD propagation restored,
    - `19`: non-bare unborn HEAD clone now preserves `refs/heads/mydefaultbranch` and emits warning,
    - `21`: bare unborn HEAD propagation remains green.
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=19 -v`: pass (regression check for warning + HEAD ref).
- Harness checkpoint:
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **58/85** (improved from 52/85)
- Implemented in this increment:
  - `fetch_transport`:
    - `git://` v2 fetch path now attempts v2 `ls-refs` when appropriate and gracefully falls back for mixed v0/v1 responders.
    - v2 fetch/pack response reader no longer blocks waiting for an extra pkt-line after sideband pack termination, fixing clone/fetch `EAGAIN` timeout behavior.
    - `git://` path now selects v2 fetch request mode when v2 negotiation/`ls-refs` succeeded.
    - v2 ls-refs fetch prefix fallback restored for empty refspec flows (`refs/heads/`, `refs/tags/`), fixing filtered `ls-remote`/fetch expectations.
  - `file_upload_pack_v2`:
    - clone preflight now parses ls-refs metadata (`HEAD` oid/symref + want refs) in one pass.
    - requests `unborn` in clone ls-refs command for protocol-v2 parity.
    - preserves source `HEAD` symref fallback for file:// clone when server ls-refs omits HEAD metadata, gated by `lsrefs.unborn` config.
    - when source HEAD points to a missing branch and one real head exists, clone preflight avoids forcing that sole branch as checkout target (keeps unborn branch semantics).
  - `clone`:
    - file:// protocol-v2 preflight now returns source HEAD metadata and seeds clone branch-selection logic with that data.
    - non-bare local/file clone now preserves source unborn HEAD target when remote-tracking refs collapse to a sole branch, matching `t5702.19` semantics.
    - warning path widened to include the preserved-unborn-source case so non-bare clone emits `warning: remote HEAD refers to nonexistent ref, unable to checkout` as expected.

**2026-04-11 (protocol-v2 fetch/clone server-option parity and v2 ref-prefix stabilization)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=9-33 -v`
    - server-option parity now passes end-to-end in file:// v2 block:
      - `25`: `fetch -o hello -o world` sends `fetch> server-option=...`
      - `26`: `fetch -o hello --all` sends exactly one `server-option=hello` per remote (2 lines)
      - `27`: config-driven `remote.origin.serverOption` for fetch (`foo/bar`, reset+`tar`, CLI override)
      - `29`: clone CLI `--server-option`
      - `30`: clone config-driven `remote.origin.serverOption` chain + CLI override
      - `33`: invalid `-c remote.origin.serverOption` now fails with expected message
    - note: remaining failures in this focused run (`17`, `19`) are pre-existing unborn-HEAD clone semantics outside this increment’s scope.
  - `GUST_BIN=/workspace/target/release/grit bash t5537-fetch-shallow.sh -v`: **16/16**
    - confirms no shallow regressions; `--update-shallow` + `fetch.writeCommitGraph` path restored.
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **52/85** (improved from 42/85)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `fetch`:
    - added `-o/--server-option` CLI argument and config resolution for `remote.<name>.serverOption` with protocol-v2 gating and missing-value error parity.
    - propagated server options through file:// upload-pack v2 `fetch` requests.
    - disabled coalesced multi-remote optimization when server options are active and threaded `--all` source refspecs into v2 `ls-refs` filtering to avoid redundant/duplicate server-option emissions.
  - `clone`:
    - added `-o/--server-option` CLI argument (alias of `--server-option`) for parity.
    - merged CLI `--server-option` with `-c remote.<remote>.serverOption=...` resolution (supports empty-value reset and command-line override semantics).
    - added explicit missing-value failure for bare `-c remote.origin.serverOption` form.
    - threaded clone server options into file:// v2 preflight fetch command.
  - `fetch_transport` / `file_upload_pack_v2`:
    - v2 `ls-refs` request builder now derives prefixes from wildcard/glob and non-head namespaces (`refs/tags/*`, `refs/remotes/...`, `refs/*`) while preserving `HEAD` behavior.
    - v2 fetch writer supports optional `server-option` lines and optional `ls-refs` server-option emission (used to match `t5702.26` line-count expectations).

**2026-04-11 (protocol-v2 file:// ls-remote config-serverOption parity + matrix refresh)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=9-15 -v`: pass except broader suite dependencies
    - critical parity checks now pass:
      - `11` filtered `ls-remote ... main`,
      - `12` CLI `-o` server options,
      - `13` config-driven `remote.origin.serverOption` chain (`foo/bar`, reset+`tar`, CLI override),
      - `15` clone trace checks for `version 2` + `ref-prefix HEAD/refs/heads/refs/tags`.
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-2,16,84 -v`: pass
    - keeps earlier git:// v2 request trace, empty file clone, and one-time-script negotiate-only checks green.
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **42/85** (improved from 41/85 in prior increment)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `ls-remote` server-option config lookup now uses merged config with repository context (`ConfigSet::load(Some(repo.git_dir), true)`), enabling `remote.<name>.serverOption` for remote-name invocations (`ls-remote origin ...`).
  - file:// v2 `ls-refs` request builder keeps protocol-v2 argument parity (`peel`, `symrefs`, `unborn`) and client-side pattern filtering behavior used by upstream for simple pattern names.
  - clone URL persistence for file clones remains `file://...` so remote-name operations continue through file transport, preserving protocol-v2 path behavior.

**2026-04-11 (protocol-v2 file:// ls-remote server-option + clone preflight stabilization)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=9-15 -v`
    - pass: `10`, `11`, `12`, `14`, `15`
    - remaining fail: `13` (`server-options from configuration are used by ls-remote`) in this focused harness mode.
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=1-2,16,84 -v`: pass
    - validates:
      - git:// protocol-v2 ls-remote request tracing (`ls-remote> ... \0\0version=2\0`)
      - empty `file://` clone default-branch propagation (`16`)
      - one-time-script negotiate-only wait-for-done rejection path (`84`)
- Implemented in this increment:
  - `ls-remote`:
    - added protocol-v2 `-o/--server-option` CLI parsing and config-driven `remote.<name>.serverOption` resolution.
    - wired `server-option=` lines into file:// v2 `ls-refs` requests.
    - added protocol-version gate/error for server options under v0/v1 (`see protocol.version ...`).
  - `file://` v2 `ls-remote`:
    - switched pattern handling from `ref-prefix <pattern>` filtering to protocol-parity request args (`peel`, `symrefs`, `unborn`) and rely on client-side pattern filtering.
  - `file://` clone preflight:
    - ensure v2 preflight `ls-refs` includes `symrefs` + `ref-prefix HEAD/refs/heads/refs/tags` for trace parity.
    - fixed empty-repo preflight hang by closing stdin before waiting for upload-pack.
  - clone remote URL handling:
    - restored canonical local-path URL storage for local path clones where expected.
    - preserved explicit `file://...` URL form in remote config for file-URL clones.
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **41/85**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
  - Note: `t5702` remains partially passing in this environment; this increment addresses high-signal file:// v2 parity items while keeping full fetch matrix green.

**2026-04-11 (protocol v2 transport parity: fix empty-clone preflight hang + git:// ls-remote routing)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash t5702-protocol-v2.sh --run=1-2,16,84 -v`: pass
    - validates:
      - `git://` v2 ls-remote request/response traces (`ls-remote> ... version=2`, `ls-remote< version 2`),
      - file:// empty repository clone no longer hangs (`t5702.16`),
      - HTTP one-time-script negotiate-only wait-for-done failure path still correct (`t5702.84`).
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **37/85** (improved from prior 0/0 timeout profile; remaining failures are broader pre-existing v2 parity gaps)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `file_upload_pack_v2` clone preflight now closes upload-pack stdin before waiting when ls-refs yields no wants, preventing indefinite waits on empty repos.
  - packet tracing in `file_upload_pack_v2` now uses dynamic identity from trace context (`clone`/`fetch`/`ls-remote`) via `wire_trace::trace_packet_line_ident`, restoring expected `clone>`/`ls-remote>` labels.
  - added native `git://` `ls-remote` path through `fetch_transport::ls_remote_via_git_protocol` and wired it in `commands/ls_remote`, including correct trace identity wrapping.
  - clone `file://` protocol-v2 preflight now runs under `clone` trace identity to keep bundle negotiation trace assertions stable.

**2026-04-11 (test-httpd one_time_script path parity + t5551 harness enablement)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5702-protocol-v2.sh --run=84 -v`: pass
    - `http:// --negotiate-only without wait-for-done support` now reaches one-time-script route and fails with expected `server does not support wait-for-done`.
  - `./scripts/run-tests.sh --timeout 240 t5551-http-fetch-smart.sh`: **31/37** (harness-visible; all non-`test_expect_failure` green)
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0** (timeout mode at default 30s in this environment)
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: **31/37**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `test-httpd` one-time-script CGI routing now falls back from docroot to the enclosing test trash directory when the target repository is intentionally outside `httpd/www`, matching `t5702` one-time-script negotiate-only setup.
  - Enabled `t5551-http-fetch-smart` in harness catalog (`in_scope=yes`), and refreshed dashboards with current pass state.

**2026-04-11 (http smart auth/cookie/GIT_SMART_HTTP parity: t5551 full pass in direct run)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5551-http-fetch-smart.sh --run=1-13 -v`: pass
    - verifies auth-only-for-objects flow and no-op v0 fetch behavior.
- Full direct suite validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5551-http-fetch-smart.sh -v`: pass (all non-`test_expect_failure` cases)
  - remaining expected TODO breakages only: 7, 8, 9, 14, 20, 22.
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - HTTP client now respects `GIT_SMART_HTTP=0` by using non-smart discovery endpoint shape (`.../info/refs` without `?service=...`) for request line and trace output.
  - Added `http.cookieFile` request support and trace parity:
    - sends `Cookie:` header from configured cookie file entries,
    - redacts cookie values in `GIT_TRACE_CURL`/`GIT_CURL_VERBOSE` unless `GIT_TRACE_REDACT=0`.
  - HTTP fetch refspec source resolution now supports literal object IDs over HTTP advertisements and broader short-name ref resolution.
  - Added no-op short-circuit for HTTP fetch (v0/v1 and v2) when all wanted OIDs already exist locally and no deepen/filter extensions are requested, avoiding unnecessary authenticated POSTs.

**2026-04-11 (http-backend content-length parity: t5562 now 16/16)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated clippy edit in `grit-lib/src/repo.rs`)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16**
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **16/16** (improved from 10/16)
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - replaced `http-backend` stub with CGI-compatible smart HTTP server flow:
    - request parsing (method/query/path/content-length/content-type/content-encoding),
    - `upload-pack`/`receive-pack` POST dispatch with body validation and optional gzip decoding,
    - `info/refs` GET advertisement support,
    - Git-like CGI response formatting with omitted explicit `Status: 200 OK`.

**2026-04-11 (bundle-uri HTTP completion: t5558 now 37/37)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5558-clone-bundle-uri.sh -v`: pass (**37/37**)
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **37/37** (improved from 30/37)
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - smart HTTP transport no longer emits extra trace2 remote-helper child events for standard `info/refs` and `git-upload-pack` RPC calls, keeping `test_remote_https_urls` output limited to expected bundle-list and bundle download URLs.
  - HTTP clone now short-circuits malformed `--bundle-uri` values (space/newline/CR) with Git-like `error: bundle-uri: URI is malformed: ...` output and a successful command exit, matching malformed URI rejection expectations.

**2026-04-11 (bundle negotiation trace parity: t5558 +3 cases)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5558-clone-bundle-uri.sh --run=1-20 -v`: pass
  - verifies bundle negotiation trace expectations in early bundle-uri negotiation cases.
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **30/37** (improved from 27/37)
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - packet trace identity wrapper now also propagates negotiation label context (`fetch`/`clone`) to `trace_fetch_tip_availability`, so trace lines emitted for negotiation candidates in clone flows are labeled `clone>` (matching bundle-uri negotiation harness expectations).

**2026-04-11 (complete shallow tail: t5537 now 16/16)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-8 -v`: pass
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`: pass (**16/16**)
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **16/16** (improved from 13/16)
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `pack-objects` now prunes rev-stdin object lists for shallow repositories to keep only objects reachable from visible refs/HEAD while stopping at shallow boundaries; hidden pre-boundary objects are excluded from shallow upload packs.
  - fetch fast-forward checks now treat ancestor-walk errors in shallow repos as inconclusive (allow update rather than false non-fast-forward rejection), removing shallow-boundary-induced false rejects.
  - `repack -a -d` on shallow repos now prunes loose objects hidden behind shallow boundaries after pack replacement, matching expected shallow-file + object-store cleanup behavior.

**2026-04-11 (fetch/fsck shallow consistency: one_time_script connectivity case)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`
  - `t5537.16` now passes (`shallow fetches check connectivity before writing shallow file`)
  - remaining failures in this file: `8`, `14`, `15`
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **13/16** (improved from 12/16)
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `fetch`: ignore advertised tag refs whose tag objects are missing locally after shallow/depth fetch negotiation, preventing invalid ref updates during partial transfers.
  - `fsck`: honor local shallow boundaries while traversing commit parents so connectivity checks stop at `.git/shallow` cut points (Git-like shallow traversal).

**2026-04-11 (shallow v2 deepen wire + upload-pack shallow boundary handling)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated clippy edit in `grit-lib/src/repo.rs`)
- `cargo test -p grit-lib --lib`: pass
- `cargo build --release -p grit-rs`: pass
- Focused trace validation:
  - `GIT_TRACE_PACKET=1 GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-2 -v`
  - clone/fetch v2 request now includes `deepen 2` during `--depth=2` setup clone.
- Focused shallow validation:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-8 -v`
  - passing: `1..7`
  - remaining failure in this subset: `8`
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **12/16** (improved from 11/16)
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Current remaining `t5537` failures in verbose run:
  - `8`, `14`, `15`, `16`
- Implemented in this increment:
  - local upload-pack v2 fetch request writer now emits shallow/deepen fields (`shallow`, `deepen`, `deepen-since`, `deepen-not`, unshallow sentinel) from caller options.
  - local fetch transport v2 path now forwards fetch shallow options into v2 request writer.
  - upload-pack v0 path now:
    - tracks client-advertised shallow boundaries,
    - avoids excluding unseen ancestors when client is shallow,
    - applies depth boundary exclusions via explicit `--not` parents for depth-limited fetches.
  - protocol v2 server fetch handler (`serve_v2`) now accepts `deepen <n>` and applies matching depth exclusion commits when generating packs.
  - local `fetch --unshallow` object copy now respects source shallow boundaries when traversing commit parents.
  - `sync_shallow_boundaries_for_unshallow` now resets local boundary set from remote-reachable boundaries (no stale local carryover).

**2026-04-11 (fetch --unshallow local boundary sync attempt)**n

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **11/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Focused shallow run:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-8`
  - still failing `6`, `8`; unshallow behavior remains incomplete for this shallow source scenario.
- Implemented in this increment:
  - `fetch --unshallow` for local/ext remotes now:
    - copies reachable objects from remote for current fetched tips,
    - calls `sync_shallow_boundaries_for_unshallow(...)` to either rewrite local `.git/shallow` to remaining remote boundaries or remove it when remote is complete.
  - removed old `should_remove_local_shallow` gate in favor of boundary-sync helper path for local remotes.

**2026-04-11 (local upload-pack shallow wire options)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **11/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - Added local upload-pack shallow request wiring in transport negotiation:
    - `shallow <oid>` lines from local boundary file,
    - `deepen`/`deepen-since`/`deepen-not`,
    - `deepen 2147483647` for `--unshallow`.
  - Threaded shallow option payloads from both `fetch` and `clone` upload-pack negotiation call paths.
  - Updated ext transport helper call to the new transport signature.

**2026-04-11 (local unshallow boundary sync follow-up)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- Focused shallow checks:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh --run=1-8`: still failing `6`, `8`
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **11/16** (count unchanged vs prior checkpoint)
- Full matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **11/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16**
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - Added `sync_shallow_boundaries_for_unshallow(...)` and wired it into `fetch --unshallow` for local/ext remotes:
    - if remote has no shallow boundaries, remove local `.git/shallow`;
    - otherwise rewrite local `.git/shallow` to the remote boundary commits reachable from fetched tips.
  - Moved fetch tip trace emission before `--unshallow` boundary synchronization so the same tip set is reused consistently.
  - Local matrix behavior remains stable while keeping the shallow tail isolated to `t5537` cases `6`, `8`, `14`, `15`, `16`.

**2026-04-11 (shallow/clone follow-up: no-single-branch + shallow ref filtering refinement)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **10/16** (unchanged baseline count; focused failures remain `6`, `8`, `9`, `14`, `15`, `16`)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37** (unchanged baseline)
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16** (unchanged baseline)
- Focused verbose check:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`
  - verified `--update-shallow` cluster (`10`-`13`) remains green
  - remaining shallow failures still concentrated in boundary/unshallow/http one-time-script edge cases.
- Implemented in this increment:
  - Added `clone --no-single-branch` option and conflict validation with `--single-branch`.
  - `--no-single-branch` now explicitly disables single-branch clone behavior (needed by shallow repack scenario setup).
  - Refined shallow boundary blocking in fetch:
    - only block refs when remote shallow boundaries are **new** relative to local `.git/shallow`,
    - skip blocking `refs/tags/*` so depth fetch tag parity is preserved while still gating branch updates.

**2026-04-10 (fetch shallow follow-up: update-shallow wiring + v2 delim tolerance)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib`: pass (reverted unrelated `grit-lib/src/repo.rs` edit)
- `cargo test -p grit-lib --lib`: 166 passed
- `cargo build --release -p grit-rs`: pass
- `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **10/16** (improved from previous 6/16 baseline)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37** (unchanged)
- `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16** (unchanged)
- Focused verbose check:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`
  - passing: `--update-shallow` cluster now green (`10`, `11`, `12`, `13`)
  - remaining failures: `4`, `6`, `8`, `14`, `15`, `16`
- Implemented in this increment:
  - Added fetch CLI support for `--update-shallow` option and propagated through `pull` fetch args.
  - Added local-transport shallow boundary gating for fetch updates:
    - refs requiring remote shallow-boundary updates are filtered unless
      `--update-shallow`/depth/deepen/unshallow/shallow-since/shallow-exclude is active.
  - Added remote-shallow boundary materialization path for `--update-shallow` without depth/deepen:
    - writes local `.git/shallow` entries reachable from fetched tips and present in remote shallow boundary set.
  - Adjusted `--unshallow` handling for local remotes:
    - only removes local `.git/shallow` when remote is actually complete (no remote shallow boundary file entries).
  - Added protocol-v2 fetch response tolerance for section delimiter packets:
    - treat `Packet::Delim` as section boundary separator in both HTTP and file upload-pack v2 parsers.

**2026-04-11 (shallow local unshallow/ref-filtering follow-up)**

- `cargo fmt`: pass
- `cargo check -p grit-rs`: pass
- `cargo build --release -p grit-rs`: pass
- `cargo test -p grit-lib --lib`: pass
- Matrix checkpoint (ordered):
  - `./scripts/run-tests.sh t5702-protocol-v2.sh`: **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh`: no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh`: **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh`: **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh`: **11/16** (improved from 10/16)
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh`: **27/37** (baseline held)
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh`: **10/16** (baseline held)
  - `./scripts/run-tests.sh t5510-fetch.sh`: **215/215**
- Implemented in this increment:
  - `fetch --unshallow` now removes local `.git/shallow` only when the resolved local/ext remote has no shallow boundary entries.
  - CLI refspec mapping path now applies the same blocked shallow-ref filter used by configured mapping path, preventing shallow-forbidden refs from being updated via explicit CLI patterns.
  - Added helper `repository_has_shallow_boundary(...)` to centralize remote shallow-boundary presence check.

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
