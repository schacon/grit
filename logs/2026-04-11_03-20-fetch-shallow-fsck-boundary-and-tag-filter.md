## Summary

Focused shallow-tail iteration for `t5537-fetch-shallow.sh`, targeting cases `8`, `14`, `15`, `16`.

## Changes implemented

### 1) Fetch: avoid updating tag refs to missing objects

- File: `grit/src/commands/fetch.rs`
- Location: `fetch_remote(...)` after remote shallow-block filtering.
- Change:
  - Opened local repository and filtered `remote_tags` to only retain tag refs whose tag object OID exists locally.
  - Rationale: during shallow/depth exchanges and manipulated HTTP responses, remote advertisement may include tag refs whose objects are not present in the received pack. Updating those refs leaves the repo inconsistent for fsck.

### 2) Fsck: respect local shallow boundaries during reachability walk

- File: `grit/src/commands/fsck.rs`
- Changes:
  - Added helper `read_shallow_boundary_oids(git_dir: &Path) -> HashSet<ObjectId>`.
  - In `walk_reachable(...)`, loaded shallow boundary OIDs once.
  - While traversing commit objects, stopped parent traversal when current commit is listed in `.git/shallow` (still traversing the commit’s tree).
  - Rationale: aligns fsck reachability with shallow graft semantics and avoids false-positive missing-parent diagnostics across shallow boundaries.

## Validation performed

- Rust quality/build gates:
  - `cargo fmt`
  - `cargo check -p grit-rs`
  - `cargo test -p grit-lib --lib`
  - `cargo build --release -p grit-rs`

- Focused suite:
  - `GUST_BIN=/workspace/target/release/grit bash tests/t5537-fetch-shallow.sh -v`
  - Result change:
    - **Case 16 now passes**.
    - Remaining failures: `8`, `14`, `15`.
    - Aggregate: `13/16` (from `12/16` baseline).

- Matrix rerun (ordered):
  1. `./scripts/run-tests.sh t5702-protocol-v2.sh` → `0/0`
  2. `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning in this harness selection
  3. `./scripts/run-tests.sh t5555-http-smart-common.sh` → `10/10`
  4. `./scripts/run-tests.sh t5700-protocol-v1.sh` → `24/24`
  5. `./scripts/run-tests.sh t5537-fetch-shallow.sh` → `13/16`
  6. `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → `27/37`
  7. `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → `10/16`
  8. `./scripts/run-tests.sh t5510-fetch.sh` → `215/215`

## Notes

- Multiple experimental attempts around `pack-objects` shallow exclusion semantics were tested and reverted due regressions; final committed delta is limited to `fetch.rs` and `fsck.rs`.
- Remaining shallow-tail failures (`8`, `14`, `15`) still center on clone/fetch object-shape parity from shallow sources and shallow-file maintenance across repack.
