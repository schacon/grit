# HTTP fetch phase C.1/C.2/C.3 — wire depth/deepen/shallow/filter options

## Scope

- Continue executing fetch-plan after Phase B transport/protocol work.
- Wire HTTP fetch request options for:
  - `--depth` / `--deepen`
  - `--shallow-since`
  - `--shallow-exclude`
  - `--filter`
- Keep HTTP v2 fast-path and HTTP v0/v1 fallback using one request-options model.

## Changes made

1. Added HTTP fetch wire-options type in `grit/src/http_smart.rs`:
   - `HttpFetchOptions { depth, deepen, shallow_since, shallow_exclude, filter_spec }`
   - helper functions for requested depth and feature/capability checks.

2. Extended v0/v1 request encoding:
   - New `append_fetch_request_extensions_v0_v1(...)` adds:
     - `deepen <n>`
     - `deepen-since <date>` when server advertises `deepen-since`
     - `deepen-not <rev>` for each `--shallow-exclude` when server advertises `deepen-not`
     - `filter <spec>` when server advertises `filter`
   - Applied to `fetch_pack_v0_v1_stateless_http(...)`.

3. Extended v2 request encoding:
   - New `append_fetch_request_extensions_v2(...)` adds:
     - `deepen <n>`
     - `deepen-since <date>`
     - `deepen-not <rev>`
     - `filter <spec>`
   - Features gated by parsed `fetch=` server feature set.
   - Applied in both initial and `done` request paths of v2 negotiation.

4. Updated API wiring:
   - `http_fetch_pack(...)` now accepts `&HttpFetchOptions`.
   - `grit/src/commands/fetch.rs` now builds and passes options from fetch args.
   - `grit/src/commands/clone.rs` passes clone depth/shallow/filter values through the same options type.

5. HTTP CLI refspec parity groundwork:
   - `fetch.rs` HTTP transport now preserves HTTP advertised refs (`adv`) for downstream mapping/updates path.
   - Existing local-only CLI refspec write block remains non-HTTP in this increment; this change sets up data needed for Phase D unification.

## Validation run

- `cargo fmt` ✅
- `cargo clippy --fix --allow-dirty -p grit-rs -p grit-lib` ✅ (reverted unrelated autofix edits before commit)
- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- `cargo build --release -p grit-rs` ✅
- `./scripts/run-tests.sh t5700-protocol-v1.sh` → `9/24` (unchanged in this environment)
- `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `13/37` (unchanged in this environment)

## Notes

- This increment wires the options into HTTP protocol messages, but full shallow/filter parity
  still depends on response-side shallow handling and broader fetch/ref update semantics in later phases.
- Bundle-uri suite remains heavily constrained by broader pre-existing transport parity gaps in this environment.
