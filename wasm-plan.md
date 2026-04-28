# Grit WASM Build Plan

Goal: build a browser-targeted WASM package that reuses the portable parts of
`grit-lib` and replaces native-only filesystem, process, environment, and
network assumptions. The target workflow is a blobless HTTP clone/fetch into a
browser, lazy promisor blob hydration, local commit creation, pack generation,
and unauthenticated smart-HTTP push where credentials are embedded in the input
URL and converted to browser-compatible request headers.

## Dependency-Ordered Tasks

### 1. Create the WASM Crate

- [x] Add a `grit-wasm` workspace member with `cdylib` output.
- [~] Add a small public API surface for browser callers: repository creation,
  object presence checks, object reads/writes, and a version/health check.
- [x] Keep the first crate build independent of browser HTTP so it can compile
  before transport extraction is complete.

### 2. Define Browser-Compatible Storage Boundaries

- [x] Add object storage traits to `grit-lib` for reading, writing, and checking
  objects without requiring a filesystem-backed `Odb`.
- [x] Add ref storage traits for `HEAD`, branch refs, remote-tracking refs, and
  advertised remote refs.
- [x] Add pack storage traits for raw pack bytes, pack index metadata, and
  promisor markers.
- [x] Implement adapters from the existing native `Odb`/refs code to the new
  traits so native behavior is preserved.
- [x] Implement an in-memory store in `grit-wasm` for early tests.
- [x] Implement persistent browser storage after the in-memory flow works:
  IndexedDB snapshots for refs/staging/promisor metadata plus raw pack and
  object bytes.

### 3. Make Core Object Operations Storage-Agnostic

- [x] Refactor tree writing (`write_tree`) to accept object storage traits.
- [x] Refactor commit creation helpers to write through object storage traits.
- [x] Refactor pack unpacking so fetched packs can write to any object store.
- [x] Preserve the native `Odb` API as a compatibility adapter while migrated
  call sites move to traits.

### 4. Extract Git Wire Protocol Helpers

- [x] Move pkt-line helpers from the binary crate into reusable library code.
- [x] Move sideband encode/decode helpers into reusable library code.
- [x] Extract upload-pack discovery and request/response builders from
  `grit/src/http_smart.rs` without carrying native HTTP, tracing, or env logic.
- [x] Extract receive-pack advertisement, request building, sideband decoding,
  and report-status parsing from `grit/src/http_push_smart.rs`.
- [x] Keep transport code out of these helpers: they should consume and produce
  bytes, URLs, content types, and parsed protocol structs.

### 5. Implement Browser HTTP Transport

- [x] Add an async `fetch`-based HTTP backend in `grit-wasm`.
- [x] Parse URL userinfo and convert it to an `Authorization` header, then send
  requests to the same URL without userinfo.
- [x] Send `Git-Protocol: version=2` when protocol v2 is requested.
- [~] Surface CORS and authentication failures with actionable errors.
- [~] Support smart HTTP endpoints:
  `GET /info/refs?service=git-upload-pack` (implemented for `ls_refs`),
  `POST /git-upload-pack` (implemented for `ls_refs`),
  `GET /info/refs?service=git-receive-pack`, and
  `POST /git-receive-pack`.

### 6. Implement Blobless Fetch and Clone

- [x] Discover refs with protocol v2 `ls-refs`.
- [x] Build upload-pack fetch requests with `filter blob:none`.
- [x] Parse sideband responses and extract pack bytes.
- [x] Store fetched packs as promisor packs.
- [x] Initially unpack fetched packs into the object store for simplicity.
- [x] Update `HEAD`, local branch refs, remote-tracking refs, and promisor
  metadata.
- [x] Add an API for `clone_blobless(url, branch_or_ref)`.

### 7. Implement Promisor Object Hydration

- [x] Detect missing promised objects during blob reads.
- [x] Fetch explicit missing object IDs through upload-pack.
- [x] Store hydrated objects in the object store.
- [x] Add an API for reading a file by path that lazily fetches its blob.
- [x] Add an API for fetching arbitrary missing object IDs.

### 8. Implement Browser Commit Creation

- [x] Add a browser index model or a storage-backed equivalent for staged file
  entries.
- [x] Write blobs from JS-provided file bytes and modes.
- [x] Build trees from the staged entries.
- [x] Serialize commits using caller-provided author, committer, message,
  parent IDs, and timestamps.
- [x] Update the current branch ref after a successful commit.
- [x] Avoid implicit current time in core code; timestamps come from the caller.

### 9. Implement Pack Writing for Push

- [x] Add a pure Rust PACK v2 writer to `grit-lib`.
- [x] Start with non-delta full-object packs for correctness.
- [x] Walk objects reachable from the new commit.
- [x] Exclude objects reachable from the remote old tip.
- [x] Write pack headers, zlib-compressed object entries, and trailing SHA-1.
- [ ] Add optional thin-pack and delta support later using existing delta helpers.

### 10. Implement HTTP Push

- [x] Discover receive-pack refs and capabilities.
- [x] Build push commands from local ref updates.
- [x] Generate pack data using the new pack writer.
- [x] Send receive-pack requests with `report-status` and sideband capabilities.
- [x] Parse unpack/ref update status and surface per-ref failures.
- [x] Update remote-tracking refs after successful pushes.
- [x] Add an API for `push(url, local_ref, remote_ref)`.

### 11. Add Tests and Browser Harnesses

- [x] Add unit tests for storage traits, pkt-line, sideband, upload-pack request
  bytes, receive-pack request bytes, and pack writer output.
- [x] Add `wasm-bindgen-test` coverage for the in-memory store and exported
  browser APIs.
- [x] Add HTTP fixture tests using recorded smart-HTTP advertisements and pack
  responses.
- [x] Add browser integration tests behind a local CORS-enabled Git HTTP test
  server.
- [x] Add a static browser example that blobless-clones a remote and displays
  recent commit messages.

### 12. Optimize for Real Browser Repositories

- [ ] Stop exploding every fetched pack into loose objects once pack indexes work
  in browser storage.
- [ ] Add streaming pack ingestion where browser APIs make it practical.
- [ ] Add progress callbacks for fetch, lazy blob hydration, pack writing, and
  push.
- [ ] Add cancellation support through `AbortController`.
- [x] Add storage quota reporting and cleanup APIs.

## First MVP

The first useful end-to-end MVP is intentionally simple:

1. `grit-wasm` compiles and exposes an in-memory repository.
2. Protocol v2 upload-pack requests can be built for `filter blob:none`.
3. Browser `fetch` can download a pack and unpack it into memory.
4. Missing blobs can be fetched explicitly by object ID.
5. Commits can be created from JS-provided file contents.
6. A non-delta full-object pack can be generated.
7. Receive-pack push can send that pack and parse `report-status`.

## Browser Integration Test

The opt-in browser network test is `grit-wasm/tests/browser_network.rs`. To run
it, start a smart HTTP server with CORS enabled, then compile/run the wasm tests
with `GRIT_WASM_TEST_REMOTE_URL` set to a served repository URL:

```bash
target/debug/test-httpd --root /tmp/git-http-root --cors --port 0
GRIT_WASM_TEST_REMOTE_URL=http://127.0.0.1:<port>/smart/repo.git \
  wasm-pack test --chrome --headless grit-wasm
```
