# Plan: `git fetch` — gaps vs upstream Git (library-oriented)

This document compares Grit’s **`grit fetch`** implementation to upstream Git’s transport and fetch machinery, and lists **missing or incomplete work in dependency order**. HTTP(S) smart transport is the main network path; **dumb HTTP** and **git://** are called out separately.

---

## 1. Current state (Grit)

### 1.1 Module intent (`grit/src/commands/fetch.rs`)

- Header still says **“only local (file://)”**, but the implementation **does** support HTTP(S) via **`http_smart::http_fetch_pack`**, plus **`ext::`**, **`git://`** (delegates to real git), **SSH** (local git dir only), and **file** paths with rich refspec / ref-update / `FETCH_HEAD` behavior.

### 1.2 What works today

| Transport | Behavior |
|-------------|----------|
| **Local path / `file://`** | Full upload-pack negotiation or object copy, **CLI refspecs** (including globs, OIDs, negated patterns), **`--refetch`**, coalesced remotes, **prefetch** namespace, etc. |
| **`git://`** | Delegates to **system `git`** (`transport_passthrough`). |
| **`ext::`** | `fetch_via_ext_skipping` when resolvable. |
| **SSH** | Only if URL resolves to a **local** git dir (`ssh_transport`); otherwise errors. |
| **HTTP(S)** | **`http_fetch_pack`**: **protocol v2** only (`command=ls-refs`, `command=fetch`), **`SkippingNegotiator`**, sideband pack unpack. **`fetch.bundleURI`** applied before fetch (`bundle_uri`) for bundle-uri tests. |
| **Bundle file as `remote.*.url`** | No-op fetch succeeds (`t5605`). |

### 1.3 HTTP implementation details (`grit/src/http_smart.rs` + `http_client.rs`)

- **Hard dependency on v2**: after stripping the v0 smart prefix, **`read_v2_caps` requires `version 2`**. Servers that only speak **v0/v1** smart HTTP (no v2) are **unsupported**; Git uses **`fetch-pack --stateless-rpc`** and handles v0/v1/v2 discovery.
- **`Git-Protocol: version=2`** is sent on **every** GET/POST via `HttpClientContext` — generally aligned with **upload-pack** fetch, unlike push (where v2 must be suppressed for receive-pack).
- **Glob refspecs** on HTTP: **`collect_wants_from_advertised` explicitly bails** (`"glob refspec in HTTP fetch not supported"`). The harness documents this: **`lib-httpd.sh`** uses **real git** for HTTP fetch when refspecs contain `*` (e.g. `t5558` bundle list expansion), and otherwise may use grit for bundle-uri parity.
- **No** wire negotiation for **`depth` / `deepen` / `shallow-since` / `shallow-exclude`** in `http_fetch_pack` — Git sends shallow/deepen lines in the **fetch** command.
- **`--filter`**: only **`blob:none`** has a **post-fetch** local approximation (`apply_blob_none_filter`); **no** promisor/partial clone protocol over HTTP.
- **`--refetch`**: only affects **local object copy** path; **HTTP** does not force re-download of already-present objects in a Git-compatible way.

### 1.4 HTTP + CLI refspecs (`fetch.rs`)

- The large block that applies **explicit CLI refspecs** to refs (lines ~970–1284) is gated with **`!is_http_url`**. It requires a **local `remote_repo`** to resolve sources and write destinations.
- For **HTTP**, **`http_fetch_pack`** is still called with **CLI refspecs** as **`upload_pack_refspecs`** to drive **wants** (non-glob), but **ref updates** fall through to the **configured refspec** path (`union_refspecs`) — **not** `src:dst` from the command line for arbitrary branches.
- **Exception**: **tag** CLI refspecs can use **`remote_tags`** from HTTP when resolving `FETCH_HEAD` tag lines (~1475–1480).

### 1.5 Other flags

- **`shallow_since` / `shallow_exclude`**: appear in **`Args`** but are **not wired** in `run()`.
- **`negotiate_only`**: early-exits **before** any work (stub).
- **`jobs`**: accepted, ignored.

### 1.6 `grit ls-remote` (`grit-lib`)

- Documented as **local only**; **no** HTTP listing (Git uses same discovery as fetch).

---

## 2. Reference: how Git does HTTP fetch

- **`git remote-curl`** + **`git fetch-pack --stateless-rpc`**: **GET** `info/refs?service=git-upload-pack` with **`Git-Protocol`** per config, parse **v0/v1** ref list or **v2** capability block, then **POST** `git-upload-pack` with pkt-line **fetch** request (wants/haves/done, shallow, filter, etc.).
- **`protocol.version`**: v2 used for upload-pack when available; **fallback** to v0 when needed (see `remote-curl.c` `discover_refs`).
- **Chunked** request bodies, **gzip**, **100-continue**, **credential** retry on **401** — same family as push (see `push-plan.md` transport notes).

---

## 3. Gaps summary (ordered by theme)

| Theme | Gap |
|--------|-----|
| **Protocol** | **v2-only** client; no **v0/v1** smart HTTP fetch path. |
| **Refspecs** | **Glob** refspecs on HTTP **unsupported**; **CLI `src:dst`** branch updates **not** applied like local fetch (only configured mapping + pack wants). |
| **Shallow / depth** | **No** server negotiation in `http_fetch_pack`; local **`shallow`** file after fetch is **approximate**, not full clone semantics. |
| **Filter / promisor** | No **`filter=`** in fetch command; **`blob:none`** is local-only. |
| **Refetch** | Not honored for HTTP. |
| **Auth** | **Origin** HTTP **401** + credential helper loop **missing** (same class as push). |
| **Transport** | **Chunked POST** / **`http.postBuffer`**, **gzip** request body — **missing** in `http_client` (relevant for huge fetches). |
| **Dumb HTTP** | **`git http-fetch`** / `update-server-info` **not** implemented in grit. |
| **SSH / real network** | SSH **only** when mapped to local repo. |

---

## 4. Work plan (dependency order, library-oriented)

### Phase A — HTTP transport (shared with push)

**A.1 `Git-Protocol` and headers**

- Per-request control of **`Git-Protocol`** (fetch may keep v2 default; **discovery** must still handle servers that **only** return v0/v1 unless we implement fallback).

**A.2 Credential and 401 retry for the origin**

- Same as push: **401** → credential fill → **Authorization** retry (and store). Required for **authenticated** HTTP fetch tests.

**A.3 Large POST bodies**

- **Chunked** upload and **`http.postBuffer`**-driven behavior when request size is unpredictable or large (Git **probe** + chunked). Parity with **`t5562`**-style upload paths.

**A.4 Optional gzip of request body**

- Match Git’s **gzip** of large POST bodies when beneficial.

---

### Phase B — Protocol v0/v1 smart HTTP fetch (or unified discovery)

**B.1 Discovery**

- Parse **non-v2** `info/refs` response: **pkt-line** ref advertisement + capabilities (same family as **`send-pack`/`fetch-pack`** advertisement parsing).
- **Strip** `# service=git-upload-pack` service block for v0 smart HTTP.

**B.2 Stateless fetch RPC**

- Either implement **`fetch-pack`**-equivalent over HTTP (multipart POST loop like Git’s **`rpc_service`**) or **spawn** **`grit fetch-pack`** with **`--stateless-rpc`** if/when that plumbing exists.
- Wire **negotiation**: **have** lines, **done**, **acknowledgments** / **packfile** sections per v0/v1 rules.

**B.3 Unify with existing v2 path**

- Prefer **one** internal API: **“fetch objects for these wants”** with **protocol version** chosen from discovery + config, so **`http_fetch_pack`** becomes **v2 fast path** + **v0/v1 fallback** (mirrors Git).

---

### Phase C — Wire `fetch` features into HTTP

**C.1 Shallow / depth / deepen**

- Extend **fetch** command pkt-line body with **`deepen`, `deepen-since`, `deepen-not`, `shallow`, `filter`** as Git does, **when** `args.depth` / `args.deepen` / `shallow_*` / `filter` are set.
- Update **`$GIT_DIR/shallow`** from **server** response, not only local walk (`write_shallow_info` today).

**C.2 `shallow_since` / `shallow_exclude`**

- Implement parsing and map to **`deepen-since` / `deepen-not`** (or equivalent) in the protocol.

**C.3 Partial clone / filter**

- **`filter=blob:none`** (and other filters): send **`filter`** in fetch command, handle **promisor** oids / **filter` capability if server supports.

**C.4 `--refetch`**

- Define semantics: **re-request** pack for **tips** even if objects exist (or clear local packs — match Git); **HTTP** path must implement equivalent.

---

### Phase D — Refspec and ref-update parity over HTTP

**D.1 Glob refspecs**

- From **`ls-refs`** full list, expand **glob** sources and match destinations (same as local path using **`refs::list_refs`** on remote — **HTTP** equivalent is **advertised** ref list only).

**D.2 CLI `src:dst` refspecs**

- After pack fetch, **apply** explicit refspec mappings using **advertised** OIDs + local repo (reuse logic from the **`!is_http_url`** block, but **without** `remote_repo` disk — use **`remote_heads` / `remote_tags` / `ls-refs`** snapshot).

**D.3 Negative refspecs**

- Ensure **exclude** patterns work **HTTP** when expanding **glob** / **CLI** lists.

---

### Phase E — UX, tests, and tooling

**E.1 `negotiate_only`**

- Implement **real** negotiation (no pack) or **document** unsupported; **Git** uses this for protocol tests.

**E.2 `grit ls-remote` over HTTP**

- Reuse **discovery** + **ls-refs** (v2) or **advertisement** (v0/v1) to print refs; depends on **Phase B**.

**E.3 Trace parity**

- **`GIT_TRACE_PACKET`**, **`GIT_TRACE_CURL`**: align with **`t5551`** / **`t5555`** / **`t5700`** expectations.

**E.4 Harness**

- Reduce reliance on **hybrid wrapper** delegating to **real git** for HTTP fetch when **glob** refspecs and **v0** servers are implemented (`tests/lib-httpd.sh`).

---

### Phase F — Optional / later

**F.1 Dumb HTTP**

- **`git http-fetch`** / **`fetch-pack` from `info/refs` + pack URLs** — large, separate from smart HTTP.

**F.2 HTTP/2**

- **`t5559`**-style behavior; depends on client stack (ureq / TLS).

**F.3 Submodule `fetch.recurseSubmodules` over HTTP**

- Submodule URLs over HTTP need **working** recursive fetch (same HTTP stack).

---

## 5. Suggested module layout

| Layer | Responsibility |
|-------|----------------|
| **`grit-lib`** | Fetch negotiation state (have/done), **shallow** / **filter** request encoding, **advertisement** parsing (v0/v1), **unpack** (already partly here), **refspec** expansion from **advertised** ref list. |
| **`grit` binary** | **`http_smart`**: v2 + v0/v1 orchestration, **`HttpClientContext`** extensions, **`fetch.rs`** ref-update branches for **HTTP** + CLI refspecs. |

---

## 6. Regression targets (harness)

Prioritize when validating HTTP fetch:

- **`t5551-http-fetch-smart.sh`**, **`t5555-http-smart-common.sh`**, **`t5559-http-fetch-smart-http2.sh`** — smart HTTP, **trace**, **HTTP/2**.
- **`t5558-clone-bundle-uri.sh`** — **bundle URI** + fetch (already partially grit-driven); **glob** refspec cases still need **real git** until **D.1** lands.
- **`t5562-http-backend-content-length.sh`** — **gzip** / **content-length** for **upload** (fetch-pack).
- **`t5700-protocol-v1.sh`**, **`t5702-protocol-v2.sh`** — **Git-Protocol** header behavior for **fetch**.
- **Shallow tests** over HTTP (e.g. **`t5550`**, shallow clone + fetch) — **Phase C**.

---

## 7. Summary

**Fetch over HTTP is partially implemented** (protocol **v2** + **SkippingNegotiator** + bundle URI pre-hook). The **largest** follow-ups are: **(1)** **v0/v1** smart HTTP (or **fallback** when v2 is absent), **(2)** **CLI/glob refspec** parity over HTTP, **(3)** **shallow/deepen/filter** on the wire, **(4)** **credential** and **large POST** transport parity, **(5)** **refetch** and **negotiate-only** semantics.

**Dependency spine:** **Phase A** (transport) → **Phase B** (protocol fallback) → **Phase C** (fetch options on wire) → **Phase D** (refspecs) → **Phase E** (polish + ls-remote).

---

*Generated from `grit` `fetch`, `http_smart`, `http_client`, `tests/lib-httpd.sh`, and Git `remote-curl.c` / `fetch-pack` behavior.*
