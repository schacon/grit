## 2026-04-11 00:44 — fetch unshallow boundary sync for local remotes

### Scope
- Continue Phase C shallow parity work for `t5537-fetch-shallow.sh` tail.
- Focused on `--unshallow` behavior against local/ext remotes.

### Implementation
- Updated `grit/src/commands/fetch.rs`:
  - Moved `trace_fetch_tip_availability` earlier to keep `tip_oids` available for unshallow sync.
  - Added `sync_shallow_boundaries_for_unshallow(local_git_dir, remote_git_dir, tip_oids)`:
    - Reads remote shallow boundary OIDs.
    - If remote has no boundaries: removes local `.git/shallow`.
    - If remote has boundaries: traverses reachable graph from fetched tips in remote ODB and writes only encountered remote boundary commits into local `.git/shallow`.
  - In `fetch --unshallow`, local/ext path now:
    - `copy_reachable_objects(remote, local, tip_oids)`,
    - then `sync_shallow_boundaries_for_unshallow(...)`.
  - Non-local transport fallback still removes local shallow file (cannot inspect remote graph).

### Validation
- `cargo fmt` ✅
- `cargo check -p grit-rs` ✅
- `cargo build --release -p grit-rs` ✅
- `cargo test -p grit-lib --lib` ✅
- Matrix rerun:
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **11/16**
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **27/37**
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **10/16**
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → no-match warning in current harness selection
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → **24/24**
  - `./scripts/run-tests.sh t5510-fetch.sh` → **215/215**

### Notes
- `t5537` remained **11/16**; failing set still includes `6, 8, 14, 15, 16`.
- `t5562` remains constrained by intentionally unimplemented `grit http-backend`.
