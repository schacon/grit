## 2026-04-11 00:24 — fetch shallow parity follow-up

### Scope
- Continue remaining Phase C shallow parity tail from the plan.
- Keep regression matrix stable while iterating on `t5537-fetch-shallow.sh` failures.

### Code changes
- `grit/src/commands/fetch.rs`
  - Refined `--unshallow` behavior for local/ext transports:
    - only remove local `.git/shallow` when the inspected remote has no shallow boundary entries.
  - Applied shallow-boundary block filtering to CLI refspec mapping source (`remote_all_refs`)
    so blocked refs are excluded consistently in explicit refspec paths.
  - Added helper `repository_has_shallow_boundary(...)` for readability/reuse.

### Validation
- Build/quality:
  - `cargo fmt` ✅
  - `cargo check -p grit-rs` ✅
  - `cargo build --release -p grit-rs` ✅
  - `cargo test -p grit-lib --lib` ✅
- Matrix checkpoints:
  - `./scripts/run-tests.sh t5702-protocol-v2.sh` → **0/0**
  - `./scripts/run-tests.sh t5551-http-fetch-smart.sh` → **no-match warning in harness**
  - `./scripts/run-tests.sh t5555-http-smart-common.sh` → **10/10**
  - `./scripts/run-tests.sh t5700-protocol-v1.sh` → **24/24**
  - `./scripts/run-tests.sh t5537-fetch-shallow.sh` → **11/16** (improved from 10/16)
  - `./scripts/run-tests.sh t5558-clone-bundle-uri.sh` → **27/37** (unchanged)
  - `./scripts/run-tests.sh t5562-http-backend-content-length.sh` → **10/16** (unchanged)
  - `./scripts/run-tests.sh t5510-fetch.sh` → **215/215**

### Notes
- Remaining `t5537` failures are concentrated in deeper shallow history transfer and one-time-script
  HTTP manipulation cases; this increment improved baseline by one case without regressing other
  matrix suites.
