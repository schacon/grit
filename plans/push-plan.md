# Plan: `grit push` over HTTP(S) (smart HTTP)

This document compares Grit’s current `push` path to upstream Git’s HTTP transport and lists **missing work in dependency order**, with a **library-first** breakdown. It targets **smart HTTP** (`git-http-backend` / `POST .../git-receive-pack`), **not** legacy WebDAV (`git http-push`).

---

## 1. Current state (Grit)

### 1.1 `grit push` (`grit/src/commands/push.rs`)

- Module docs state **only local `file://` transport**; the implementation routes URLs as follows:
  - `git://` → protocol check + **delegates to real `git`**.
  - SSH (configured `ssh_transport`) → **local git dir** path only; errors if not resolvable locally.
  - Everything else → **`check_protocol_allowed("file", …)`** and **`PathBuf::from(url)`** (after optional `file://` strip). A **`https://...` URL is **not** treated as HTTP**; it becomes a nonsensical filesystem path and fails at `open_repo`.
- Full push logic (refspecs, `--mirror`, `--delete`, hooks, `--force-with-lease`, `--atomic`, `--push-option`, porcelain, submodule recursion, etc.) is implemented **against a local `Repository` opened for the “remote” path** — correct for file/SSH-local simulation only.

### 1.2 `grit send-pack` (`grit/src/commands/send_pack.rs`)

- Implements **classic pkt-line push** (v0/v1 style) by spawning **`git receive-pack`** / `grit receive-pack` on a **local path**, reading the **advertisement**, writing `old new ref` lines + caps, then streaming **`pack-objects --stdout`** into the same stdin stream.
- **No** `--stateless-rpc` / `--helper-status` (Git’s HTTP path uses these).
- **No** HTTP; **no** multi-round **POST** loop.

### 1.3 `grit http-push` (`grit/src/commands/http_push.rs`)

- **Stub**: always errors (“not yet implemented”). Upstream Git uses this only for **non-smart (dumb) WebDAV** push (`push_dav` in `remote-curl.c`), not for normal smart HTTP.

### 1.4 HTTP fetch (reference for what exists)

- **`http_smart`** (`grit/src/http_smart.rs`): **protocol v2 only** for `git-upload-pack` (`command=ls-refs`, `command=fetch`), plus sideband pack reading.
- **`http_client`** (`grit/src/http_client.rs`): GET/POST, proxy, `GIT_TRACE_CURL`, `GIT_ASKPASS` for **proxy** password — but **always sends `Git-Protocol: version=2`** on both GET and POST (see below).

### 1.5 Submodule push

- `grit-lib` `push_submodules.rs`: treats `http://` / `https://` URLs as **non-resolvable** to a local repo (`resolve_remote_url_to_local_git_dir` returns `None`), so **submodule recursion cannot push over HTTP** until URL-based transport exists.

---

## 2. Reference: how Git does HTTP push

Primary sources: `git/remote-curl.c`, `git/builtin/send-pack.c`.

### 2.1 Discovery

- **`GET $URL/info/refs?service=git-receive-pack`** (when `GIT_SMART_HTTP` is enabled).
- If `protocol.version` is **2** in config, Git **still forces v0 for receive-pack** (because v2 push does not exist): see `discover_refs` in `remote-curl.c` (`strcmp("git-upload-pack", service)` fallback).
- Response is **smart HTTP** (`application/x-git-receive-pack-advertisement`): first pkt-line `# service=git-receive-pack`, then **ref advertisement** pkt-lines (v0/v1), **not** v2 capability lines like `version 2`.
- **`Git-Protocol` HTTP header**: `version=1` or `version=0` when applicable; **must not** advertise v2 for push (tests: `t5702` “push with http:// and a config of v2 does not request v2”).

### 2.2 Push execution

- If **smart** (`proto_git`): **`push_git`** runs **`git send-pack --stateless-rpc --helper-status … $URL --stdin`** with refspec lines as packet-lines on stdin **prepended** to the **discovery buffer** (`rpc_service` writes preamble + `heads->buf` to send-pack stdin).
- **`send-pack`** with **`--stateless-rpc`**: stdin is **0**, advertisement is read from **fd 0** after refspec pkt-lines; **`discover_version`** → **v0/v1 only**; **`protocol_v2` dies** in `send-pack.c` (“not implemented yet”).
- **`rpc_service`**: reads **pkt-lines from send-pack stdout**; each non-empty chunk is **`POST $URL/git-receive-pack`** with `Content-Type: application/x-git-receive-pack-request`, `Accept: application/x-git-receive-pack-result`, optional **`Git-Protocol`** header matching discovery version, optional **gzip** body, and for large bodies **chunked transfer** + **probe** (`http_post_buffer`).
- Response: **sideband** + **report-status**; `--helper-status` controls helper-oriented status output.

### 2.3 Dumb HTTP

- If not smart: **`push_dav`** runs **`git http-push`** (WebDAV). Separate, large effort; **optional** for parity with very old servers.

---

## 3. Gaps vs Git (must-haves for smart HTTP push)

| Area | Git | Grit today |
|------|-----|------------|
| URL routing in `push` | HTTP → remote helper / curl | Treated as file path |
| `info/refs` for receive-pack | GET + v0/v1 advertisement | Only v2 upload-pack client exists |
| `Git-Protocol` on HTTP | v0/v1 for push; **no v2** for push | `http_client` **always** sends `version=2` |
| Push RPC | Multi-POST stateless RPC loop | None |
| `send-pack` | `--stateless-rpc`, stdin advertisement + refspecs | Local process only |
| Chunked POST / `http.postBuffer` | Yes | Not in `http_client` |
| Gzip request body | Often | Not |
| Auth / 401 | Credential fill + retry | No full origin HTTP Basic flow (proxy auth only) |
| `--force-with-lease` / CAS | Uses remote refs from advertisement | Needs remote refs without local remote repo |
| Submodules | Can push via HTTP URL | Skipped for HTTP URLs |

---

## 4. Work plan (dependency order, library-oriented)

### Phase A — HTTP transport primitives (blocks everything else)

**A.1 `Git-Protocol` header policy**

- Add **per-request** control of `Git-Protocol` (omit, `version=0`, `version=1`, `version=2`) on GET/POST.
- Implement **push discovery rule**: when talking to **`git-receive-pack`**, **never** request v2 (mirror Git + `t5702`).

**A.2 Discovery response parsing (v0/v1 advertisement)**

- In **`grit-lib`** (recommended): parse pkt-line advertisement after `# service=git-receive-pack` and flush: **ref lines** (`oid SP refname`), **capabilities** on first ref, **`.have` / shallow** lines as needed.
- Reuse patterns from `send_pack::read_advertisement` / `parse_advertised_ref_oid` but **input is bytes** (not a child stdout), and must handle **exactly** what `git-http-backend` emits over HTTP.

**A.3 `GET` for `receive-pack`**

- Build URL: `{base}/info/refs?service=git-receive-pack`.
- Validate **Content-Type** smart advertisement when possible (Git’s `check_smart_http` semantics).
- Return **striped advertisement buffer** (payload after service header) for feeding into push logic.

**A.4 `POST` to `git-receive-pack`**

- URL: `{base}/git-receive-pack`.
- Headers: `Content-Type` / `Accept` for `git-receive-pack` (same pattern as upload-pack in `http_smart`).
- **Chunked** `POST` when body size **exceeds** effective `http.postBuffer` (from config; Git default is 1 MiB) — **required** for `t5541` “push (chunked)”.
- Optional **gzip** of request body (Git does when beneficial) — **required** for some `t5562` paths if present in harness.
- **`Expect: 100-continue`** behavior if implementing Git-compatible auth probing (lower priority than Basic + retry).

**A.5 HTTP authentication for the **origin** (not only proxy)**

- On **401**, run credential helper flow (`git credential fill` / `grit credential` parity) and retry with `Authorization: Basic …` (and store on success if configured). **Blocks** `t5541` “push over smart http with auth” and `t5563`-style tests.

---

### Phase B — Stateless push RPC (core protocol)

**B.1 Logical “send-pack” over HTTP (library)**

- Implement a **library** module (e.g. `grit_lib::transport::http::push` or `grit_lib::send_pack_stateless`) that:
  1. Takes **local repo**, **advertisement bytes**, **resolved ref updates** (list of `old_oid`, `new_oid`, `ref_name`, caps), and options (thin, atomic, push options, report-status expectations).
  2. **Constructs** the pkt-line stream that would go to receive-pack stdin: **ref updates** + **flush** + **pack** (from `pack-objects --stdout` with same `^` tips as `send_pack.rs`).
  3. **Splits** into **POST bodies** by reading pkt-lines: send-pack **outputs one pkt-line per HTTP request** until flush (Git `rpc_service` loop). Implement the **same** chunking: buffer until flush, POST, repeat.

**B.2 Response handling**

- Parse **sideband** on the **result** stream (same as fetch path: channels 1/2/3).
- Parse **report-status** / **report-status-v2** in the sideband stream for error reporting and porcelain parity.

**B.3 Thin pack and `^` hints**

- Reuse **`send_pack::peel_advertised_commits`** / **`.have`** handling from advertisement so **thin packs** are correct (same constraints as local `send_pack`).

---

### Phase C — Wire `grit push` to HTTP

**C.1 `push_to_url` branch**

- If URL is `http://` or `https://`:
  - `check_protocol_allowed("http"` / `"https")` (or whatever names match Git’s `protocol.*` keys).
  - Build `HttpClientContext`, run **Phase A discovery**, then **Phase B push**.
- **Do not** `open_repo` for the URL string.

**C.2 Reuse refspec → `RefUpdate` resolution**

- The **bulk** of `push.rs` already computes **`RefUpdate`** from local repo + **remote** ref OIDs. For HTTP, **remote** OIDs come from **advertisement** (and possibly from `refs/remotes/...` for lease — Git uses remote-tracking refs + advertisement); thread **advertised refs** into the same validation functions where possible instead of `refs::resolve_ref` on a remote path.

**C.3 Hooks**

- **pre-push**: needs **remote** ref OIDs for each line; supply from **advertisement** (and local refs as today). **Hooks may run before** the first POST (same ordering as Git).

**C.4 `--receive-pack` and system `git`**

- Today: if `--receive-pack` is set, `push` delegates to **system `git`**. Keep that behavior for HTTP if desired, or **error** when combined with native HTTP push — pick one and document (Git allows custom helper path).

---

### Phase D — Feature parity & polish

**D.1 Config knobs**

- `http.postBuffer`, `protocol.version` (push path must **fall back** from v2), `http.receivepack` / `receivepack` server-side is server-only; client: `http.version` / SSL if ureq exposes them.

**D.2 Trace / UX**

- `GIT_TRACE_CURL`: ensure **POST** lines match Git shape (`t5541`: `POST git-receive-pack (N bytes)` vs chunked).
- `GIT_TRACE_PACKET` / wire trace parity where tests assert.

**D.3 `--porcelain`, `--quiet`, `--progress`**

- Map to sideband progress channel and report-status parsing (already partially mirrored in local push).

**D.4 Submodule recursion (`--recurse-submodules`)**

- **After** URL-based push works: extend `push_submodules` to call **HTTP push** into submodule URLs (or explicitly **fail** with a clear message until implemented).

---

### Phase E — Optional / later

**E.1 WebDAV / `grit http-push`**

- Implement dumb HTTP push or document **unsupported**; Git uses `git http-push` only when `proto_git` is false.

**E.2 Protocol v2 push**

- Git **does not** support it yet; **no work** until upstream adds it.

---

## 5. Suggested module layout

| Layer | Responsibility |
|-------|----------------|
| **`grit-lib`** | Advertisement parsing, ref matching inputs, thin-pack `^` set, packfile generation orchestration (or thin wrapper over existing `pack-objects` spawn), report-status parsing, types (`AdvertisedRef`, `HttpPushResult`). |
| **`grit` binary** | `HttpClientContext`, chunked/gzip POST, credential retry, `push_to_url` dispatch, trace2 `child_start` parity with `http_smart`, CLI wiring. |

Keeping **HTTP I/O in the binary** matches existing `http_smart` / `http_client` placement while **pkt-line semantics and pack construction** live in **`grit-lib`** for tests and reuse.

---

## 6. Regression targets (harness)

Prioritize when validating:

- `t5541-http-push-smart.sh` — smart push, **chunked** POST, **auth**, **atomic**, hooks.
- `t5542-push-http-shallow.sh` — shallow + HTTP.
- `t5545-push-options.sh` — `--push-option` over HTTP.
- `t5548-push-porcelain.sh` — HTTP porcelain cases.
- `t5562-http-backend-content-length.sh` — gzip / content-length edge cases.
- `t5700-protocol-v1.sh` / `t5702-protocol-v2.sh` — **Git-Protocol** header behavior for push.

---

## 7. Summary

**Critical path:** **(A)** HTTP client fixes (protocol header, discovery GET, chunked POST, auth) → **(B)** stateless send-pack RPC (pack + multi-POST) → **(C)** integrate with `push.rs` ref resolution using **advertised** refs instead of a local remote repo.

**Largest design risk:** **reusing** vs **forking** `send_pack.rs`: Git runs **`send-pack` as a subprocess** with stdin = advertisement + refspecs; Grit can either **port that orchestration** into a library or **spawn `grit send-pack` with extended flags** once `--stateless-rpc` exists — **library-first implementation** is the long-term fit for `AGENTS.md`, but **spawning** a thin internal `send-pack` mode may be faster to land if stdin/stdout plumbing is identical to Git.

---

*Generated from a read-through of `grit` `push`, `send_pack`, `http_smart`, `http_client`, and Git `remote-curl.c` / `send-pack.c`.*
