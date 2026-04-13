## Phase E.2 — `ls-remote` over HTTP parity

### Goal

Wire `grit ls-remote` to use existing smart-HTTP discovery/ls-refs plumbing so HTTP(S) remotes are supported via internal transport code instead of local-path fallback.

### Implementation

- Updated `grit/src/commands/ls_remote.rs`:
  - Detect HTTP(S) repository arguments early.
  - Build HTTP client context from merged config (`ConfigSet::load(None, true)` + command `-c`/env via existing config path).
  - Call `crate::http_smart::http_ls_refs(repo_url, &http_ctx)` for HTTP(S).
  - Convert `LsRefEntry` results into `RefEntry` so existing output/filter/sort rendering is reused.
  - Keep existing `file://` v2 path and upload-pack fallback unchanged.

### Validation

- `cargo check -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- Manual e2e parity check with local `test-httpd`:
  - Created temporary repo + mirror under `/tmp/grit-lsremote-http-test`.
  - Started `target/release/test-httpd` on `127.0.0.1:33357`.
  - Compared:
    - `grit -c protocol.version=2 ls-remote http://127.0.0.1:33357/smart/repo.git`
    - `git  -c protocol.version=2 ls-remote http://127.0.0.1:33357/smart/repo.git`
  - `diff -u` produced no differences ✅
- Regression suites rerun after this increment:
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → `0/0`
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → `9/24`
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `13/37`
  - No regressions relative to current baseline.
