## Phase slice
- Continued fetch plan execution on refspec/prune parity and upload-pack negotiation behavior.

## Code changes

### 1) Upload-pack `collect_wants` now supports glob sources
- File: `grit/src/fetch_transport.rs`
- Updated `collect_wants(...)` to expand `*` in positive refspec sources against advertised refs.
- Behavior details:
  - Negative refspec tokens (`^...`) are skipped in wants collection.
  - Glob source refspecs (e.g. `refs/heads/*`) now gather matching advertised OIDs.
  - If a pattern has no matches, it no longer hard-fails with
    `glob refspec in upload-pack fetch not supported`.
- Result:
  - Removed a large class of hard failures in `t5510-fetch.sh` prune/tag matrix.

### 2) Upload-pack wants-empty path no longer hard-fails for CLI pattern mismatches
- File: `grit/src/fetch_transport.rs`
- In `fetch_via_upload_pack_skipping(...)`, when `wants` is empty:
  - now drains child output and returns advertised heads/tags instead of bailing for CLI refspec mode.
- This allows namespace-only/prune-only fetches with unmatched patterns to proceed and still evaluate prune/update behavior.

### 3) Refspec-scoped remote-tracking prune
- File: `grit/src/commands/fetch.rs`
- Added helpers:
  - `prune_prefixes_from_cli_refspecs(...)`
  - `prune_prefixes_from_fetch_refspecs(...)`
- Prune flow now computes prune namespaces from destination mappings instead of always pruning full `refs/remotes/<remote>/`.
- This aligns better with tests expecting:
  - `git fetch --prune origin main` not to prune unrelated remote-tracking branches.
  - namespace-scoped `src:dst` prune to affect only mapped destinations.

## Validation

- Build/tests:
  - `cargo fmt`
  - `cargo check -p grit-rs`
  - `cargo build --release -p grit-rs`
  - `cargo test -p grit-lib --lib` (166 passed)

- Focused fetch suite:
  - `./scripts/run-tests.sh t5510-fetch.sh`
    - Improved to **90/215** (from **86/215** immediately prior and much lower earlier in session).

- Regression matrix rerun:
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → 20/24
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → 27/37
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → 6/16
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → 10/16
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → 10/10
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → 0/0 (harness timeout mode in this run)

## Notes
- The highest remaining `t5510` failure clusters are now concentrated around:
  - full prune policy matrix edge-cases,
  - `--atomic` transaction semantics,
  - `--refmap` option behavior,
  - dry-run / write-fetch-head interactions,
  - a handful of bundle and connectivity corner cases.
